//! Typed-contract macro behavior that crosses the RPC and runtime boundary.

use near_kit::ContractError;
use near_kit::protocol::Action;
use near_kit::transaction::{CodecError, Final, FunctionCall};
use near_kit::{AccountId, Gas, Near, NearToken, SigningNear};
use near_kit_sandbox::Sandbox;
use serde::{Deserialize, Serialize};

use super::support::{deploy_ft_contract, funded_account, guestbook_wasm, shared_client};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GuestbookMessage {
    pub premium: bool,
    pub sender: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AddMessageArgs {
    pub text: String,
}

#[near_kit::contract]
pub trait Guestbook {
    fn total_messages(&self) -> u32;
    fn get_messages(&self) -> Vec<GuestbookMessage>;

    #[call]
    fn add_message(&mut self, args: AddMessageArgs);
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
        .add_message(AddMessageArgs {
            text: "typed message".to_owned(),
        })
        .await
        .expect("typed call")
        .result()
        .unwrap();

    // Gas and deposit set on the call reach the contract.
    guestbook
        .add_message(AddMessageArgs {
            text: "premium".to_owned(),
        })
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
        .add_message(AddMessageArgs {
            text: "hello".to_owned(),
        })
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
