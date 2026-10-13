//! Integration test for transfers to `0u` universal accounts (nearcore 2.14).
//!
//! What a plain transfer to a `0u` id does depends on how the node was built:
//!
//! * Universal accounts enabled (`wasm_config.universal_accounts`): the
//!   transfer creates the account *uninitialized*. `view_account` reports
//!   `state: "uninitialized"` plus the `bootstrap_nonce` its self-signed
//!   `UniversalStateInit` must carry. The `2.14.0-rc.3` sandbox enables them at
//!   protocol v87, so this is the branch the default image runs.
//! * Universal accounts disabled at protocol v87+: nearcore's
//!   `RejectUniversalAccountTransfers` ban (near/nearcore#16520) rejects the
//!   transaction with `ActionsValidationError::TransferToUniversalAccountNotAllowed`.
//!
//! The test reads the node's own protocol config to pick the branch, then
//! asserts that branch strictly. A node that predates both panics.
//!
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

use near_kit::*;
use near_kit::{protocol::*, rpc::*, transaction::Final};

use super::support::shared_client;

/// A valid `0u` universal account id (its state init is never applied here).
const UNIVERSAL_ACCOUNT_ID: &str = "0u000g40r40m30e209185gr38e1w8124gk2gahc5rr34d1p70x3rfg";

/// Protocol version that introduced universal accounts and the transfer ban
/// (nearcore 2.14).
const UNIVERSAL_ACCOUNTS_PROTOCOL_VERSION: u32 = 87;

/// nearcore's `AccessKey::ACCESS_KEY_NONCE_RANGE_MULTIPLIER`: a bootstrap nonce
/// is `(block_height - 1) * 1_000_000`, like a new access key's nonce.
const ACCESS_KEY_NONCE_RANGE_MULTIPLIER: u64 = 1_000_000;

#[tokio::test]
async fn test_transfer_to_universal_account() {
    let (_sandbox, root_near) = shared_client().await;
    let account_id: AccountId = UNIVERSAL_ACCOUNT_ID.parse().expect("valid 0u account id");

    let status = root_near.rpc().status().await.expect("status");
    let protocol_config: serde_json::Value = root_near
        .rpc()
        .call(
            "EXPERIMENTAL_protocol_config",
            serde_json::json!({ "finality": "final" }),
        )
        .await
        .expect("EXPERIMENTAL_protocol_config");
    let universal_accounts_enabled =
        protocol_config["runtime_config"]["wasm_config"]["universal_accounts"]
            .as_bool()
            .unwrap_or(false);

    let amount = NearToken::from_near(1);

    if universal_accounts_enabled {
        // The id doesn't exist until the transfer funds it.
        let missing = root_near
            .rpc()
            .view_account(&account_id, BlockReference::Finality(Finality::Final))
            .await
            .expect_err("0u account must not exist before the transfer");
        assert!(
            missing.is_account_not_found(),
            "expected AccountNotFound before the transfer, got {missing:?}"
        );

        let outcome = root_near
            .transaction(&account_id)
            .transfer(amount)
            .send()
            .wait_until::<Final>()
            .await
            .expect("transfer to a 0u account must be accepted");
        assert!(
            outcome.is_success(),
            "transfer to a 0u account must succeed: {outcome:?}"
        );

        let view = root_near
            .rpc()
            .view_account(&account_id, BlockReference::Finality(Finality::Final))
            .await
            .expect("view the funded 0u account");
        assert_eq!(view.state, AccountState::Uninitialized);
        assert!(!view.state.is_initialized());
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
            .transaction(&account_id)
            .transfer(amount)
            .send()
            .wait_until::<Final>()
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
