//! Typed-contract macro behavior that crosses the RPC and runtime boundary.

use near_kit::rpc::Finality;
use near_kit::transaction::Final;
use near_kit::{AccountId, Error, Gas, Near, NearToken};
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

async fn deploy_guestbook(prefix: &str) -> (&'static Sandbox, Near, AccountId) {
    let (sandbox, root) = shared_client().await;
    let (contract, contract_id, _) =
        funded_account(&root, sandbox, prefix, NearToken::from_near(20)).await;
    contract
        .deploy(guestbook_wasm())
        .send()
        .wait_until::<Final>()
        .await
        .expect("guestbook deployment");
    (sandbox, root, contract_id)
}

#[tokio::test]
async fn typed_views_and_calls_roundtrip() {
    let (_, near, contract_id) = deploy_guestbook("typed").await;
    let guestbook = near
        .contract::<Guestbook>(&contract_id)
        .expect("typed client");

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
        .send()
        .wait_until::<Final>()
        .await
        .expect("typed call");

    let messages = guestbook.get_messages().await.expect("updated messages");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].text, "typed message");
}

#[tokio::test]
async fn typed_call_configuration_reaches_the_contract() {
    let (_, near, contract_id) = deploy_guestbook("typedcfg").await;
    let guestbook = near
        .contract::<Guestbook>(&contract_id)
        .expect("typed client");

    guestbook
        .add_message(AddMessageArgs {
            text: "premium".to_owned(),
        })
        .gas(Gas::from_tgas(50))
        .deposit(NearToken::from_near(1))
        .send()
        .wait_until::<Final>()
        .await
        .expect("configured typed call");

    let messages = guestbook
        .get_messages()
        .finality(Finality::Optimistic)
        .await
        .expect("configured typed view");
    assert_eq!(messages.len(), 1);
    assert!(messages[0].premium);
}

#[tokio::test]
async fn typed_views_work_without_a_signer_but_calls_do_not() {
    let (sandbox, _, contract_id) = deploy_guestbook("typednosigner").await;
    let no_signer = Near::custom(sandbox.rpc_url(), sandbox.chain_id().clone()).build();
    let guestbook = no_signer
        .contract::<Guestbook>(&contract_id)
        .expect("typed client");

    assert_eq!(guestbook.total_messages().await.expect("unsigned view"), 0);
    let error = guestbook
        .add_message(AddMessageArgs {
            text: "cannot send".to_owned(),
        })
        .await
        .expect_err("call without signer must fail");
    assert!(matches!(error, Error::NoSigner));
}

#[tokio::test]
async fn typed_views_preserve_decode_and_block_errors() {
    #[near_kit::contract]
    trait WrongGuestbook {
        fn total_messages(&self) -> String;
    }

    let (_, near, contract_id) = deploy_guestbook("typederrors").await;
    let wrong = near
        .contract::<WrongGuestbook>(&contract_id)
        .expect("wrong typed client");
    assert!(matches!(wrong.total_messages().await, Err(Error::Json(_))));

    let guestbook = near
        .contract::<Guestbook>(&contract_id)
        .expect("typed client");
    assert!(matches!(
        guestbook.total_messages().at_block(999_999_999_u64).await,
        Err(Error::Rpc(_))
    ));
}
