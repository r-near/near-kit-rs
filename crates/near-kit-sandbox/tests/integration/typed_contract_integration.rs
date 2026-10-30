//! Typed-contract macro behavior that crosses the RPC and runtime boundary.

use near_kit::ContractError;
use near_kit::protocol::Action;
use near_kit::rpc::{ActionView, FinalExecutionOutcome};
use near_kit::transaction::{CodecError, Final, FunctionCall};
use near_kit::{AccountId, Gas, Near, NearToken, SigningNear};
use near_kit_sandbox::Sandbox;
use serde::{Deserialize, Serialize};

use super::support::{
    deploy_ft_contract, funded_account, guestbook_wasm, shared_client, unique_account,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuestbookMessage {
    pub premium: bool,
    pub sender: String,
    pub text: String,
}

#[near_kit::contract]
pub trait Guestbook {
    fn total_messages(&self) -> u32;
    fn get_messages(&self) -> Vec<GuestbookMessage>;

    #[call]
    fn add_message(&mut self, text: String);
}

/// The guestbook's `get_messages(from_index: Option<U128>, limit: Option<U64>)`
/// with its parameters, sent by name.
#[near_kit::contract]
pub trait PagedGuestbook {
    fn get_messages(&self, from_index: Option<&str>, limit: Option<&str>) -> Vec<GuestbookMessage>;
}

/// Deploy a guestbook; returns the root account waiting for finality, so a
/// typed call is visible to the default (final) read that follows it.
async fn deploy_guestbook(prefix: &str) -> (&'static Sandbox, SigningNear<Final>, AccountId) {
    let (sandbox, root) = shared_client().await;
    let (contract, contract_id, _) =
        funded_account(&root, sandbox, prefix, NearToken::from_near(20)).await;
    contract
        .wait_until::<Final>()
        .transaction(contract.account_id())
        .add_action(Action::deploy_contract(guestbook_wasm()))
        .await
        .expect("guestbook deployment")
        .result()
        .unwrap();
    (sandbox, root.wait_until::<Final>(), contract_id)
}

#[tokio::test]
async fn typed_views_and_calls_roundtrip() {
    let (sandbox, near, contract_id) = deploy_guestbook("typed").await;
    let guestbook = near.contract::<Guestbook>(&contract_id);

    // Views need no signer. (A `#[call]` on a client over a read-only `Near`
    // doesn't compile; near-kit-macros' `compile-fail/call_without_sender`
    // pins that.)
    let no_signer: Near = sandbox.root_account().near().clone();
    let unsigned = no_signer.contract::<Guestbook>(&contract_id);
    assert_eq!(unsigned.total_messages().await.expect("unsigned view"), 0);
    assert!(
        guestbook
            .get_messages()
            .await
            .expect("initial messages")
            .is_empty()
    );

    guestbook
        .add_message("typed message".to_owned())
        .await
        .expect("typed call")
        .result()
        .unwrap();

    // Gas and deposit set on the call reach the contract.
    guestbook
        .add_message("premium".to_owned())
        .gas(Gas::from_tgas(50))
        .deposit(NearToken::from_near(1))
        .await
        .expect("configured typed call")
        .result()
        .unwrap();

    let messages = guestbook.get_messages().await.expect("updated messages");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].text, "typed message");
    assert!(!messages[0].premium);
    assert!(messages[1].premium);

    // Two parameters, by name; `None` is `null`, which near-sdk reads as
    // `None`.
    let paged = near.contract::<PagedGuestbook>(&contract_id);
    let page = paged
        .get_messages(Some("1"), Some("1"))
        .await
        .expect("paged view");
    assert_eq!(page, messages[1..]);
    let all = paged.get_messages(None, None).await.expect("unpaged view");
    assert_eq!(all, messages);
}

/// A near-sdk token's methods declared with their parameters, which the
/// contract reads by name, and one with an `#[args]` struct.
#[near_kit::contract]
pub trait NamedToken {
    fn ft_balance_of(&self, account_id: &AccountId) -> String;

    #[call(deposit = NearToken::from_millinear(10))]
    fn storage_deposit(&mut self, #[args] args: StorageDepositArgs);

    #[call(deposit = NearToken::from_yoctonear(1))]
    fn ft_transfer(&mut self, receiver_id: &AccountId, amount: &str, memo: Option<&str>);
}

#[derive(Serialize)]
pub struct StorageDepositArgs {
    pub account_id: AccountId,
    pub registration_only: bool,
}

#[tokio::test]
async fn named_parameters_reach_a_near_sdk_contract() {
    let (_, root) = shared_client().await;
    let root = root.wait_until::<Final>();
    let (ft_id, _) = deploy_ft_contract(&root, root.account_id())
        .await
        .expect("token deployment");
    let token = root.contract::<NamedToken>(&ft_id);
    let bob = unique_account("bob");

    token
        .storage_deposit(StorageDepositArgs {
            account_id: bob.clone(),
            registration_only: true,
        })
        .await
        .expect("sent")
        .result()
        .expect("registered");
    token
        .ft_transfer(&bob, "1000", Some("named"))
        .await
        .expect("sent")
        .result()
        .expect("transferred");

    assert_eq!(token.ft_balance_of(&bob).await.expect("view"), "1000");
    assert_eq!(
        token.ft_balance_of(root.account_id()).await.expect("view"),
        "999999999999999999999000"
    );
}

#[tokio::test]
async fn typed_views_preserve_decode_and_block_errors() {
    #[near_kit::contract]
    trait WrongGuestbook {
        fn total_messages(&self) -> String;
    }

    let (_, near, contract_id) = deploy_guestbook("typederrors").await;
    let wrong = near.contract::<WrongGuestbook>(&contract_id);
    assert!(matches!(
        wrong.total_messages().await,
        Err(ContractError::Codec(CodecError::Json(_)))
    ));

    let guestbook = near.contract::<Guestbook>(&contract_id);
    assert!(matches!(
        guestbook.total_messages().at(999_999_999_u64).await,
        Err(ContractError::Backend(_))
    ));
}

/// Every way of making a JSON view or call without arguments sends `{}`. A
/// near-sdk method whose parameters are all `Option` (the guestbook's
/// `get_messages(from_index, limit)`, the token's `storage_withdraw(amount)`)
/// parses that object; empty bytes fail to parse.
#[tokio::test]
async fn no_argument_views_and_calls_send_an_empty_json_object() {
    let (_, near, contract_id) = deploy_guestbook("noargs").await;
    let reader = near.near();
    let guestbook = near.contract::<Guestbook>(&contract_id);
    guestbook
        .add_message("hello".to_owned())
        .await
        .expect("typed call")
        .result()
        .unwrap();

    let typed = guestbook.get_messages().await.expect("typed view");
    let untyped: Vec<GuestbookMessage> = reader
        .view(&contract_id, "get_messages")
        .await
        .expect("near.view without arguments");
    assert_eq!(typed, untyped);
    assert_eq!(untyped.len(), 1);
    // A method without parameters doesn't read its input, so `{}` is fine.
    let total: u32 = reader.view(&contract_id, "total_messages").await.unwrap();
    assert_eq!(total, 1);

    let empty = reader
        .view::<Vec<GuestbookMessage>>(&contract_id, "get_messages")
        .args_raw(Vec::new())
        .await
        .expect_err("empty bytes aren't a JSON object");
    assert!(
        format!("{empty:?}").contains("Failed to deserialize input from JSON"),
        "{empty:?}"
    );

    // The token registers its owner, so the owner can withdraw (nothing).
    let (_, root) = shared_client().await;
    let root = root.wait_until::<Final>();
    let (ft_id, _) = deploy_ft_contract(&root, root.account_id())
        .await
        .expect("token deployment");
    let withdraw = || FunctionCall::new("storage_withdraw").deposit(NearToken::from_yoctonear(1));
    root.transaction(&ft_id)
        .add_action(withdraw())
        .await
        .expect("storage_withdraw without arguments")
        .result()
        .expect("`{}` parses as all-`None` arguments");
    let failed = root
        .transaction(&ft_id)
        .add_action(withdraw().args_raw(Vec::new()))
        .await
        .expect("sent")
        .result()
        .expect_err("empty bytes aren't a JSON object");
    assert!(
        format!("{failed:?}").contains("Failed to deserialize input from JSON"),
        "{failed:?}"
    );
}

/// NEP-145's `storage_withdraw`, which (like most token methods) requires
/// exactly 1 yoctoNEAR.
#[near_kit::contract]
pub trait StorageWithdraw {
    #[call(gas = Gas::from_tgas(40), deposit = NearToken::from_yoctonear(1))]
    fn storage_withdraw(&mut self);
}

/// The gas and deposit of the transaction's only action, as signed.
fn sent_gas_and_deposit(outcome: &FinalExecutionOutcome) -> (Gas, NearToken) {
    match &outcome.transaction.actions[..] {
        [ActionView::FunctionCall { gas, deposit, .. }] => (*gas, *deposit),
        other => panic!("expected one function call, got {other:?}"),
    }
}

/// `#[call(gas = .., deposit = ..)]` reaches the chain without `.gas()` or
/// `.deposit()` at the call site, through the client and through a batched
/// constructor, and the call site still overrides it.
#[tokio::test]
async fn call_defaults_reach_the_chain_and_the_call_site_overrides_them() {
    let (_, root) = shared_client().await;
    let root = root.wait_until::<Final>();
    // The token registers its owner, so the owner can withdraw (nothing).
    let (ft_id, _) = deploy_ft_contract(&root, root.account_id())
        .await
        .expect("token deployment");
    let token = root.contract::<StorageWithdraw>(&ft_id);

    let outcome = token.storage_withdraw().await.expect("sent");
    outcome.result().expect("1 yoctoNEAR attached by default");
    assert_eq!(
        sent_gas_and_deposit(&outcome),
        (Gas::from_tgas(40), NearToken::from_yoctonear(1))
    );

    let outcome = token
        .storage_withdraw()
        .gas(Gas::from_tgas(50))
        .await
        .expect("sent");
    outcome.result().expect("overridden gas");
    assert_eq!(
        sent_gas_and_deposit(&outcome),
        (Gas::from_tgas(50), NearToken::from_yoctonear(1))
    );

    let outcome = token
        .storage_withdraw()
        .deposit(NearToken::from_yoctonear(2))
        .await
        .expect("sent");
    assert_eq!(
        sent_gas_and_deposit(&outcome),
        (Gas::from_tgas(40), NearToken::from_yoctonear(2))
    );
    let failed = outcome
        .result()
        .expect_err("the contract takes exactly 1 yoctoNEAR");
    assert!(
        format!("{failed:?}").contains("Requires attached deposit of exactly 1 yoctoNEAR"),
        "{failed:?}"
    );

    let outcome = root
        .transaction(&ft_id)
        .add_action(StorageWithdraw::storage_withdraw())
        .await
        .expect("sent");
    outcome.result().expect("batched with the defaults");
    assert_eq!(
        sent_gas_and_deposit(&outcome),
        (Gas::from_tgas(40), NearToken::from_yoctonear(1))
    );
}
