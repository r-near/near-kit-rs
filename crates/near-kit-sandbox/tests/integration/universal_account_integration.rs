//! `0u` universal accounts (nearcore 2.14). If the node enables
//! `universal_accounts`, a transfer creates an uninitialized account and a
//! state init sets it up; otherwise (protocol 87+) the transfer is rejected
//! with `TransferToUniversalAccountNotAllowed`.
//!
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

use near_kit::protocol::Action;
use near_kit::*;
use near_kit::{protocol::*, rpc::*, transaction::Final};

use near_kit::signer::{InMemorySigner, SecretKey};

use super::support::shared_client;

/// A valid `0u` universal account id (its state init is never applied here).
const UNIVERSAL_ACCOUNT_ID: &str = "0u000g40r40m30e209185gr38e1w8124gk2gahc5rr34d1p70x3rfg";

/// Protocol version that introduced universal accounts and the transfer ban
/// (nearcore 2.14).
const UNIVERSAL_ACCOUNTS_PROTOCOL_VERSION: u32 = 87;

/// nearcore's `AccessKey::ACCESS_KEY_NONCE_RANGE_MULTIPLIER`: a bootstrap nonce
/// is `(block_height - 1) * 1_000_000`, like a new access key's nonce.
const ACCESS_KEY_NONCE_RANGE_MULTIPLIER: u64 = 1_000_000;

/// Whether the node runs with `universal_accounts` enabled.
async fn universal_accounts_enabled(near: &SigningNear) -> bool {
    let protocol_config: serde_json::Value = near
        .near()
        .rpc()
        .request(
            "EXPERIMENTAL_protocol_config",
            serde_json::json!({ "finality": "final" }),
        )
        .await
        .expect("EXPERIMENTAL_protocol_config");
    protocol_config["runtime_config"]["wasm_config"]["universal_accounts"]
        .as_bool()
        .unwrap_or(false)
}

/// A key-only state init for a fresh key.
fn key_only_state_init() -> (UniversalStateInit, AccountId, SecretKey) {
    let key = SecretKey::generate_ed25519();
    let state_init: UniversalStateInit = UniversalStateInitV1::new()
        .with_access_key(key.public_key())
        .into();
    let account_id = state_init.derive_account_id();
    (state_init, account_id, key)
}

/// Assert `account_id` is initialized and `key` is one of its access keys.
async fn assert_initialized_with(near: &SigningNear, account_id: &AccountId, key: &SecretKey) {
    let view = near
        .near()
        .rpc()
        .view_account(account_id, BlockReference::Finality(Finality::Final))
        .await
        .expect("view the 0u account");
    assert_eq!(view.state, AccountState::Initialized);
    assert_eq!(view.bootstrap_nonce, None);
    near.near()
        .rpc()
        .view_access_key(
            account_id,
            &key.public_key(),
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("the state init installs its access key");
}

#[tokio::test]
async fn test_universal_state_init_by_another_account() {
    let (_sandbox, root_near) = shared_client().await;
    if !universal_accounts_enabled(&root_near).await {
        eprintln!("skipping: the sandbox does not enable universal_accounts");
        return;
    }
    let (state_init, account_id, key) = key_only_state_init();

    let outcome = root_near
        .wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::universal_state_init(
            state_init,
            NearToken::from_millinear(100),
        ))
        .await
        .expect("state init sent by another account");
    assert!(outcome.is_success(), "{outcome:?}");
    assert_initialized_with(&root_near, &account_id, &key).await;
}

#[tokio::test]
async fn test_universal_account_bootstraps_itself() {
    let (_sandbox, root_near) = shared_client().await;
    if !universal_accounts_enabled(&root_near).await {
        eprintln!("skipping: the sandbox does not enable universal_accounts");
        return;
    }
    let (state_init, account_id, key) = key_only_state_init();

    root_near
        .wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await
        .expect("fund the 0u id")
        .result()
        .unwrap();

    // Plain `send`: the nonce comes from `bootstrap_nonce`, since the key is
    // not on chain until the state init applies.
    let signer = InMemorySigner::new(account_id.clone(), key.clone());
    let outcome = root_near
        .near()
        .with_signer(signer)
        .wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::universal_state_init(state_init, NearToken::ZERO))
        .await
        .expect("self-signed state init");
    assert!(outcome.is_success(), "{outcome:?}");
    assert_initialized_with(&root_near, &account_id, &key).await;
}

#[tokio::test]
async fn test_transfer_to_universal_account() {
    let (_sandbox, root_near) = shared_client().await;
    let account_id: AccountId = UNIVERSAL_ACCOUNT_ID.parse().expect("valid 0u account id");

    let status = root_near.near().rpc().status().await.expect("status");
    let amount = NearToken::from_near(1);

    if universal_accounts_enabled(&root_near).await {
        // The id doesn't exist until the transfer funds it.
        let missing = root_near
            .near()
            .rpc()
            .view_account(&account_id, BlockReference::Finality(Finality::Final))
            .await
            .expect_err("0u account must not exist before the transfer");
        assert!(
            missing.is_account_not_found(),
            "expected AccountNotFound before the transfer, got {missing:?}"
        );

        let outcome = root_near
            .wait_until::<Final>()
            .transaction(&account_id)
            .add_action(Action::transfer(amount))
            .await
            .expect("transfer to a 0u account must be accepted");
        assert!(
            outcome.is_success(),
            "transfer to a 0u account must succeed: {outcome:?}"
        );

        let view = root_near
            .near()
            .rpc()
            .view_account(&account_id, BlockReference::Finality(Finality::Final))
            .await
            .expect("view the funded 0u account");
        assert_eq!(view.state, AccountState::Uninitialized);
        assert_eq!(
            view.amount, amount,
            "the whole deposit lands on the account"
        );
        assert!(
            view.code_hash.is_zero(),
            "an uninitialized account has no code"
        );
        let bootstrap_nonce = view
            .bootstrap_nonce
            .expect("an uninitialized universal account reports a bootstrap_nonce");
        assert!(bootstrap_nonce > 0, "bootstrap_nonce must be set");
        assert_eq!(
            bootstrap_nonce % ACCESS_KEY_NONCE_RANGE_MULTIPLIER,
            0,
            "bootstrap_nonce is derived from the creation block height"
        );
    } else {
        assert!(
            status.protocol_version >= UNIVERSAL_ACCOUNTS_PROTOCOL_VERSION,
            "sandbox runs protocol v{} without universal accounts; a 0u transfer is only \
             meaningful from v{UNIVERSAL_ACCOUNTS_PROTOCOL_VERSION} (nearcore 2.14)",
            status.protocol_version,
        );

        let error = root_near
            .wait_until::<Final>()
            .transaction(&account_id)
            .add_action(Action::transfer(amount))
            .await
            .expect_err("a transfer to a 0u account must be rejected during the ban");
        match error {
            Error::InvalidTx(invalid_tx) => match *invalid_tx {
                InvalidTxError::ActionsValidation(
                    ActionsValidationError::TransferToUniversalAccountNotAllowed {
                        account_id: rejected,
                    },
                ) => assert_eq!(rejected, account_id),
                other => panic!("expected TransferToUniversalAccountNotAllowed, got {other:?}"),
            },
            other => panic!("expected Error::InvalidTx, got {other:?}"),
        }
    }
}
