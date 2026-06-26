//! Integration test for gas-key transacting (protocol 2.13, `GasKeys` feature).
//!
//! Exercises the full RS-2 path on a real 2.13 sandbox:
//! 1. Create a funded account with an ordinary full-access key.
//! 2. Add a gas key (`AddKey` with `GasKeyFullAccess`) and fund it
//!    (`TransferToGasKey`).
//! 3. Read the gas key's parallel nonce via `EXPERIMENTAL_view_gas_key_nonces`.
//! 4. Build a `TransactionV1` whose nonce is a `GasKeyNonce { nonce, nonce_index }`,
//!    sign it with the gas key, and submit it with `send_tx`.
//! 5. Assert the transfer landed and the gas key's nonce advanced.
//!
//! Pins the sandbox to `2.13.0-rc.2` so it does not depend on the default-version
//! bump landing first. Runs against a fresh, isolated sandbox.
//!
//! Run with: `cargo test --test integration --features sandbox gas_key`

use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::sandbox::{SANDBOX_ROOT_ACCOUNT, SandboxConfig};
use near_kit::*;

/// The 2.13 sandbox image this test is written against.
const SANDBOX_VERSION: &str = "2.13.0-rc.2";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

fn unique_account(prefix: &str) -> AccountId {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}{}.{}", prefix, n, SANDBOX_ROOT_ACCOUNT)
        .parse()
        .unwrap()
}

/// Read a gas key's parallel nonces via `EXPERIMENTAL_view_gas_key_nonces`.
///
/// near-kit does not yet have a typed wrapper for this query (it lands with the
/// RPC track), so the test calls the generic JSON-RPC method directly.
async fn view_gas_key_nonces(
    near: &Near,
    account_id: &AccountId,
    public_key: &PublicKey,
) -> Vec<u64> {
    #[derive(serde::Deserialize)]
    struct Resp {
        nonces: Vec<u64>,
    }
    let params = serde_json::json!({
        "request_type": "view_gas_key_nonces",
        "finality": "final",
        "account_id": account_id.to_string(),
        "public_key": public_key.to_string(),
    });
    let resp: Resp = near
        .rpc()
        .call("query", params)
        .await
        .expect("view_gas_key_nonces query failed");
    resp.nonces
}

#[tokio::test]
async fn test_gas_key_signed_transaction() {
    // Pin to a known 2.13 image; fresh + isolated so this WP isn't blocked on the
    // default-version bump.
    let sandbox = SandboxConfig::builder()
        .version(SANDBOX_VERSION)
        .fresh()
        .await;
    let root_near = sandbox.client();

    // --- 1. Owner account with an ordinary full-access key. -----------------
    let owner_key = SecretKey::generate_ed25519();
    let owner_id = unique_account("gkowner");
    root_near
        .transaction(&owner_id)
        .create_account()
        .transfer(NearToken::from_near(20))
        .add_full_access_key(owner_key.public_key())
        .send()
        .wait_until(Final)
        .await
        .expect("create owner account");

    let owner_near = root_near.with_signer(
        InMemorySigner::from_secret_key(owner_id.clone(), owner_key.clone()).expect("owner signer"),
    );

    // --- 2. Add a gas key (GasKeyFullAccess) and fund it. -------------------
    let gas_key = SecretKey::generate_ed25519();
    let gas_pk = gas_key.public_key();
    const NUM_NONCES: u16 = 4;
    const NONCE_INDEX: u16 = 0;

    let add_gas_key = Action::AddKey(AddKeyAction {
        public_key: gas_pk.clone(),
        access_key: AccessKey {
            nonce: 0,
            permission: AccessKeyPermission::GasKeyFullAccess(GasKeyInfo {
                balance: NearToken::ZERO,
                num_nonces: NUM_NONCES,
            }),
        },
    });

    owner_near
        .transaction(&owner_id)
        .add_action(add_gas_key)
        .add_action(Action::transfer_to_gas_key(
            gas_pk.clone(),
            NearToken::from_near(5),
        ))
        .send()
        .wait_until(Final)
        .await
        .expect("add + fund gas key");

    // --- 3. Read the gas key's nonce (it is height-derived, not 0). ---------
    let nonces = view_gas_key_nonces(&root_near, &owner_id, &gas_pk).await;
    assert_eq!(
        nonces.len(),
        NUM_NONCES as usize,
        "gas key should expose {NUM_NONCES} parallel nonces, got {nonces:?}"
    );
    let start_nonce = nonces[NONCE_INDEX as usize];

    // A recent block hash. view_access_key on the gas key returns one.
    let ak = root_near
        .rpc()
        .view_access_key(
            &owner_id,
            &gas_pk,
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("view gas key access key");
    let block_hash = ak.block_hash;

    // --- 4. Build, sign (with the gas key), and submit a V1 transfer. -------
    let recipient_id = unique_account("gkrecipient");
    let recipient_key = SecretKey::generate_ed25519();
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

    let transfer_amount = NearToken::from_near(2);
    let v1 = TransactionV1 {
        signer_id: owner_id.clone(),
        public_key: gas_pk.clone(),
        nonce: TransactionNonce::from_nonce_and_index(start_nonce + 1, NONCE_INDEX),
        receiver_id: recipient_id.clone(),
        block_hash,
        actions: vec![Action::transfer(transfer_amount)],
        nonce_mode: TransactionNonceMode::Monotonic,
    };
    let signed = v1.sign(&gas_key);

    // Sanity: it really is a tagged V1 on the wire.
    assert_eq!(
        signed.to_bytes()[0],
        1,
        "gas-key transaction must serialize as a tagged V1"
    );

    // Submit via the generic send_tx RPC (V1-aware path).
    let send_params = serde_json::json!({
        "signed_tx_base64": signed.to_base64(),
        "wait_until": "FINAL",
    });
    let resp: serde_json::Value = root_near
        .rpc()
        .call("send_tx", send_params)
        .await
        .expect("send gas-key signed transaction");

    // The transaction must have executed successfully (no failure status).
    let status = &resp["final_execution_status"];
    assert!(
        resp.get("status").is_some() || status.is_string() || status.is_object(),
        "unexpected send_tx response: {resp}"
    );
    assert!(
        resp["status"].get("Failure").is_none(),
        "gas-key transaction failed on-chain: {resp}"
    );

    // --- 5. Assert effects: recipient credited, gas-key nonce advanced. -----
    let recipient_after = root_near
        .balance(&recipient_id)
        .await
        .expect("recipient balance after")
        .total;
    assert!(
        recipient_after.as_yoctonear()
            >= recipient_before.as_yoctonear() + transfer_amount.as_yoctonear(),
        "recipient should have received the transfer: before={recipient_before}, after={recipient_after}"
    );

    let nonces_after = view_gas_key_nonces(&root_near, &owner_id, &gas_pk).await;
    assert_eq!(
        nonces_after[NONCE_INDEX as usize],
        start_nonce + 1,
        "gas key nonce at the used index must have advanced to start+1"
    );
    // Other parallel nonces are untouched.
    for i in 0..NUM_NONCES as usize {
        if i != NONCE_INDEX as usize {
            assert_eq!(nonces_after[i], nonces[i], "untouched nonce {i} changed");
        }
    }
}
