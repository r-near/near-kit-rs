//! Typed-contract macro behavior that crosses the RPC and runtime boundary.

use near_kit::ContractError;
use near_kit::protocol::Action;
use near_kit::transaction::{CodecError, Final};
use near_kit::{AccountId, Gas, Near, NearToken, SigningNear};
use near_kit_sandbox::Sandbox;
use serde::{Deserialize, Serialize};

use super::support::{funded_account, guestbook_wasm, shared_client};

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
        .expect("guestbook deployment");
    (sandbox, root.wait_until::<Final>(), contract_id)
}

#[tokio::test]
async fn typed_views_and_calls_roundtrip() {
    let (_, near, contract_id) = deploy_guestbook("typed").await;
    let guestbook = near.contract::<Guestbook>(&contract_id);

    assert_eq!(guestbook.total_messages().await.expect("initial count"), 0);
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
        .expect("typed call");

    let messages = guestbook.get_messages().await.expect("updated messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "typed message");
}

#[tokio::test]
async fn typed_call_configuration_reaches_the_contract() {
    let (_, near, contract_id) = deploy_guestbook("typedcfg").await;
    let guestbook = near.contract::<Guestbook>(&contract_id);

    guestbook
        .add_message(AddMessageArgs {
            text: "premium".to_owned(),
        })
        .gas(Gas::from_tgas(50))
        .deposit(NearToken::from_near(1))
        .await
        .expect("configured typed call");

    let messages = guestbook
        .get_messages()
        .await
        .expect("configured typed view");
    assert_eq!(messages.len(), 1);
    assert!(messages[0].premium);
}

// A `#[call]` on a client over a read-only `Near` no longer compiles (pinned by
// near-kit-macros' `compile-fail/call_without_sender`), so only the view half
// of this behaviour is a runtime test.
#[tokio::test]
async fn typed_views_work_without_a_signer() {
    let (sandbox, _, contract_id) = deploy_guestbook("typednosigner").await;
    let no_signer: Near = sandbox.root_account().near().clone();
    let guestbook = no_signer.contract::<Guestbook>(&contract_id);

    assert_eq!(guestbook.total_messages().await.expect("unsigned view"), 0);
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
