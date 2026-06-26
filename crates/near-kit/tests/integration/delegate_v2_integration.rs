//! Integration test for `Action::DelegateV2` (gas-key meta-transactions, NEP-611).
//!
//! Exercises the full RS-3 relay path on a real 2.13 sandbox:
//! 1. A sender signs a `DelegateActionV2` (a transfer) under the V2 NEP-461
//!    domain, using an ordinary full-access key (plain `TransactionNonce`).
//! 2. A relayer wraps it in `Action::DelegateV2` and submits it, paying gas.
//! 3. Assert the transfer landed on-chain.
//!
//! Pins the sandbox to `2.13.0-rc.2` (protocol v85 enables `DelegateV2`).
//!
//! Run with: `cargo test --test integration --features sandbox delegate_v2`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

const SANDBOX_VERSION: &str = "2.13.0-rc.2";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account(prefix: &str) -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}{}.{}", prefix, n, SANDBOX_ROOT_ACCOUNT)
        .parse()
        .unwrap()
}

#[tokio::test]
async fn test_delegate_v2_relay() {
    let sandbox = SandboxConfig::builder()
        .version(SANDBOX_VERSION)
        .fresh()
        .await;
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
        .wait_until(Final)
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
        .wait_until(Final)
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
        .wait_until(Final)
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
