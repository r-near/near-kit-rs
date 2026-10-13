//! Integration test for `Action::DelegateV2` (gas-key meta-transactions, NEP-611).
//!
//! Exercises the full RS-3 relay path on a real 2.13 sandbox:
//! 1. A sender signs a `DelegateActionV2` (a transfer) under the V2 NEP-461
//!    domain, using an ordinary full-access key (plain `TransactionNonce`).
//! 2. A relayer wraps it in `Action::DelegateV2` and submits it, paying gas.
//! 3. Assert the transfer landed on-chain.
//!
//! The relay test pins the sandbox to stable `2.13.4`: protocol v85 enabled
//! `DelegateV2`, and nearcore 2.14 (protocol v87, `RejectDelegateV2`) removed
//! it again, so the relay path only exists on 2.13 nodes. A second test runs
//! the same relay against the default (2.14) sandbox and asserts the node
//! rejects it with `ActionsValidationError::RemovedProtocolFeature`.
//!
//! Run with: `cargo test -p near-kit-sandbox --features integration-tests --test integration`

use near_kit::*;
use near_kit::{protocol::*, rpc::*, signer::*, transaction::Final};

use super::support::{sandbox_2_13, shared_client, unique_account};

/// Protocol version at which nearcore 2.14 rejects `Action::DelegateV2`
/// (`ProtocolFeature::RejectDelegateV2`).
const REJECT_DELEGATE_V2_PROTOCOL_VERSION: u32 = 87;

#[tokio::test]
async fn test_delegate_v2_relay() {
    let sandbox = sandbox_2_13().await;
    let root_near = sandbox.client();

    // --- Accounts: sender (signs the meta-tx), relayer (pays gas), recipient.
    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("dv2sender");
    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("create sender");

    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("dv2relayer");
    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("create relayer");

    let recipient_key = SecretKey::generate_ed25519();
    let recipient_id = unique_account("dv2recipient");
    root_near
        .transaction(&recipient_id)
        .create_account()
        .transfer(NearToken::from_near(1))
        .add_full_access_key(recipient_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("create recipient");

    let recipient_before = root_near
        .balance(&recipient_id)
        .await
        .expect("recipient balance before")
        .total;

    // --- SENDER: build + sign a DelegateActionV2 (transfer to recipient). ----
    let ak = root_near
        .rpc()
        .view_access_key(
            &sender_id,
            &sender_key.public_key(),
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("view sender access key");
    let status = root_near.rpc().status().await.expect("status");
    let max_block_height = status.sync_info.latest_block_height + 200;

    let transfer_amount = NearToken::from_near(2);
    let delegate_v2 = DelegateActionV2 {
        sender_id: sender_id.clone(),
        receiver_id: recipient_id.clone(),
        actions: vec![
            NonDelegateAction::from_action(Action::transfer(transfer_amount))
                .expect("transfer is not a delegate"),
        ],
        // Ordinary full-access key → plain nonce (ak.nonce + 1).
        nonce: TransactionNonce::from_nonce(ak.nonce + 1),
        max_block_height,
        public_key: sender_key.public_key(),
    };
    let payload: VersionedDelegateActionPayload = delegate_v2.into();
    let signature = sender_key.sign(payload.get_hash().as_bytes());
    let signed = payload.sign(signature);

    // The signature verifies under the V2 domain before we even send it.
    assert!(signed.verify(), "V2 delegate signature must verify locally");

    // Round-trip the payload the way a relayer would receive it (base64).
    let received = VersionedSignedDelegateAction::from_base64(&signed.to_base64())
        .expect("relayer decodes payload");
    assert_eq!(received, signed);

    // --- RELAYER: wrap in Action::DelegateV2 and submit, paying gas. ---------
    let relayer_near = root_near.with_signer(
        InMemorySigner::from_secret_key(relayer_id.clone(), relayer_key.clone())
            .expect("relayer signer"),
    );

    // Sign the outer relay tx and submit it. We submit via the generic
    // `send_tx` RPC and inspect the raw response: the high-level
    // `FinalExecutionOutcome` parser lives in the RPC track's `rpc.rs` and does
    // not yet model the v4 execution metadata these receipts carry, so the raw
    // path is what definitively proves on-chain acceptance here.
    let signed_outer = relayer_near
        .transaction(&sender_id) // outer receiver must be the meta-tx sender
        .add_action(Action::delegate_v2(received))
        .sign()
        .await
        .expect("sign outer relay tx");
    let raw: serde_json::Value = root_near
        .rpc()
        .call(
            "send_tx",
            serde_json::json!({
                "signed_tx_base64": signed_outer.to_base64(),
                "wait_until": "FINAL",
            }),
        )
        .await
        .expect("relay DelegateV2 via send_tx");

    // The outer transaction (the DelegateV2 action) must not have failed, and
    // every receipt outcome must be a success.
    assert!(
        raw["status"].get("Failure").is_none(),
        "DelegateV2 relay transaction failed on-chain: {raw}"
    );
    let receipts = raw["receipts_outcome"]
        .as_array()
        .expect("receipts_outcome array");
    assert!(!receipts.is_empty(), "expected receipt outcomes: {raw}");
    for r in receipts {
        assert!(
            r["outcome"]["status"].get("Failure").is_none(),
            "a DelegateV2 receipt failed: {r}"
        );
    }

    // --- VERIFY the transfer landed. ----------------------------------------
    let recipient_after = root_near
        .balance(&recipient_id)
        .await
        .expect("recipient balance after")
        .total;
    let diff = recipient_after.as_yoctonear() - recipient_before.as_yoctonear();
    assert_eq!(
        diff,
        transfer_amount.as_yoctonear(),
        "recipient should have received exactly the delegated transfer"
    );
}

#[tokio::test]
async fn test_delegate_v2_rejected_as_removed_protocol_feature() {
    let (sandbox, root_near) = shared_client().await;

    let status = root_near.rpc().status().await.expect("status");
    assert!(
        status.protocol_version >= REJECT_DELEGATE_V2_PROTOCOL_VERSION,
        "default sandbox runs protocol v{}, but DelegateV2 is only rejected from \
         v{REJECT_DELEGATE_V2_PROTOCOL_VERSION} (nearcore 2.14)",
        status.protocol_version,
    );

    let sender_key = SecretKey::generate_ed25519();
    let sender_id = unique_account("dv2rmsender");
    root_near
        .transaction(&sender_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(sender_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("create sender");

    let relayer_key = SecretKey::generate_ed25519();
    let relayer_id = unique_account("dv2rmrelayer");
    root_near
        .transaction(&relayer_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(relayer_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("create relayer");

    // A well-formed, correctly signed DelegateActionV2: the only reason for the
    // node to reject it is that the action itself was removed.
    let ak = root_near
        .rpc()
        .view_access_key(
            &sender_id,
            &sender_key.public_key(),
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("view sender access key");
    let delegate_v2 = DelegateActionV2 {
        sender_id: sender_id.clone(),
        receiver_id: relayer_id.clone(),
        actions: vec![
            NonDelegateAction::from_action(Action::transfer(NearToken::from_near(1)))
                .expect("transfer is not a delegate"),
        ],
        nonce: TransactionNonce::from_nonce(ak.nonce + 1),
        max_block_height: status.sync_info.latest_block_height + 200,
        public_key: sender_key.public_key(),
    };
    let payload: VersionedDelegateActionPayload = delegate_v2.into();
    let signature = sender_key.sign(payload.get_hash().as_bytes());
    let signed = payload.sign(signature);
    assert!(signed.verify(), "V2 delegate signature must verify locally");

    let relayer_near = sandbox.client().with_signer(
        InMemorySigner::from_secret_key(relayer_id.clone(), relayer_key.clone())
            .expect("relayer signer"),
    );
    let error = relayer_near
        .transaction(&sender_id)
        .add_action(Action::delegate_v2(signed))
        .send()
        .wait_until::<Final>()
        .await
        .expect_err("a 2.14 node must reject Action::DelegateV2");

    match error {
        Error::InvalidTx(invalid_tx) => match *invalid_tx {
            InvalidTxError::ActionsValidation(ActionsValidationError::RemovedProtocolFeature {
                protocol_feature,
                version,
            }) => {
                assert_eq!(protocol_feature, "DelegateV2");
                assert_eq!(version, status.protocol_version);
            }
            other => panic!("expected RemovedProtocolFeature, got {other:?}"),
        },
        other => panic!("expected Error::InvalidTx, got {other:?}"),
    }
}
