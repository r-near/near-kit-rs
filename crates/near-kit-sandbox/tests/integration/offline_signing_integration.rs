//! One complete air-gapped signing and submission path against a real node.

use near_kit::NearToken;
use near_kit::protocol::SignedTransaction;
use near_kit::rpc::{BlockReference, Finality};
use near_kit::transaction::Final;

use super::support::{funded_account, shared_client};

#[tokio::test]
async fn offline_signed_transaction_roundtrips_and_executes() {
    let (sandbox, root) = shared_client().await;
    let (sender, sender_id, sender_key) =
        funded_account(&root, sandbox, "offline", NearToken::from_near(20)).await;
    let (_, receiver_id, _) =
        funded_account(&root, sandbox, "offlinerecv", NearToken::from_near(1)).await;
    let before = root.balance(&receiver_id).await.expect("receiver balance");

    let access_key = root
        .rpc()
        .view_access_key(
            &sender_id,
            &sender_key.public_key(),
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("access key");
    let block = root
        .rpc()
        .block(BlockReference::Finality(Finality::Final))
        .await
        .expect("final block");

    let signed = sender
        .transfer(&receiver_id, NearToken::from_near(3))
        .sign_offline(block.header.hash, access_key.nonce + 1)
        .await
        .expect("offline signature");

    let from_bytes = SignedTransaction::from_bytes(&signed.to_bytes()).expect("bytes roundtrip");
    let from_base64 =
        SignedTransaction::from_base64(&signed.to_base64()).expect("base64 roundtrip");
    assert_eq!(from_bytes, signed);
    assert_eq!(from_base64, signed);

    let outcome = sender
        .send(&from_base64)
        .wait_until::<Final>()
        .await
        .expect("offline-signed submission");
    assert!(outcome.is_success());

    let after = root.balance(&receiver_id).await.expect("receiver balance");
    assert_eq!(
        after.total.as_yoctonear() - before.total.as_yoctonear(),
        NearToken::from_near(3).as_yoctonear()
    );
}
