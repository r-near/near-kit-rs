//! Pre-signed transactions against a real node: one signed by the client,
//! one built and signed air-gapped, both submitted with `send_signed`.

use near_kit::NearToken;
use near_kit::protocol::Action;
use near_kit::protocol::{SignedTransaction, Transaction};
use near_kit::rpc::{BlockReference, Finality};
use near_kit::transaction::Final;

use super::support::{funded_account, shared_client};

#[tokio::test]
async fn pre_signed_transactions_roundtrip_and_execute() {
    let (sandbox, root) = shared_client().await;
    let (sender, sender_id, sender_key) =
        funded_account(&root, sandbox, "offline", NearToken::from_near(20)).await;
    let (_, receiver_id, _) =
        funded_account(&root, sandbox, "offlinerecv", NearToken::from_near(1)).await;
    let before = root
        .near()
        .balance(&receiver_id)
        .await
        .expect("receiver balance");

    let signed = sender
        .sign(
            &receiver_id,
            vec![Action::transfer(NearToken::from_near(2))],
        )
        .await
        .expect("signed transaction");
    // Balance queries use final state, so wait for the transfer to become final.
    let outcome = sender
        .near()
        .send_signed(&signed)
        .wait_until::<Final>()
        .await
        .expect("pre-signed submission");
    assert!(outcome.is_success());

    let access_key = root
        .near()
        .rpc()
        .view_access_key(
            &sender_id,
            &sender_key.public_key(),
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("access key");
    let block = root
        .near()
        .rpc()
        .block(BlockReference::Finality(Finality::Final))
        .await
        .expect("final block");

    // The air-gapped half needs only near-kit-primitives: no client, no RPC.
    let signed = Transaction::new(
        sender_id.clone(),
        sender_key.public_key(),
        access_key.nonce + 1,
        receiver_id.clone(),
        block.header.hash,
        vec![Action::transfer(NearToken::from_near(3))],
    )
    .sign(&sender_key);

    let from_bytes = SignedTransaction::from_bytes(&signed.to_bytes()).expect("bytes roundtrip");
    let from_base64 =
        SignedTransaction::from_base64(&signed.to_base64()).expect("base64 roundtrip");
    assert_eq!(from_bytes, signed);
    assert_eq!(from_base64, signed);

    let outcome = sender
        .near()
        .send_signed(&from_base64)
        .wait_until::<Final>()
        .await
        .expect("offline-signed submission");
    assert!(outcome.is_success());

    let after = root
        .near()
        .balance(&receiver_id)
        .await
        .expect("receiver balance");
    assert_eq!(
        after.total.as_yoctonear() - before.total.as_yoctonear(),
        NearToken::from_near(5).as_yoctonear()
    );
}
