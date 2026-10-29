//! Integration test for `Action::DelegateV2` (gas-key meta-transactions, NEP-611).
//!
//! Exercises the full RS-3 relay path on a real 2.13 sandbox:
//! 1. A sender signs a `DelegateActionV2` (a transfer) under the V2 NEP-461
//!    domain, using an ordinary full-access key (plain `TransactionNonce`).
//! 2. A relayer wraps it in `Action::DelegateV2` and submits it, paying gas.
//! 3. Assert the transfer landed on-chain.
//!
//! The relay test pins `2.13.4` (DelegateV2 exists only at protocol 85-86);
//! the rejection test runs on the default 2.14 sandbox.

use near_kit::protocol::Action;
use near_kit::*;
use near_kit::{protocol::*, rpc::*, signer::*, transaction::Final};

use super::support::{funded_account, sandbox_2_13, shared_client};

/// Protocol version at which nearcore 2.14 rejects `Action::DelegateV2`
/// (`ProtocolFeature::RejectDelegateV2`).
const REJECT_DELEGATE_V2_PROTOCOL_VERSION: u32 = 87;

/// A `DelegateActionV2` transferring `amount` from `sender` to `receiver`,
/// signed with `key`'s next plain nonce.
async fn sign_delegate_v2(
    near: &Near,
    key: &SecretKey,
    sender: &AccountId,
    receiver: &AccountId,
    amount: NearToken,
    max_block_height: u64,
) -> VersionedSignedDelegateAction {
    let ak = near
        .rpc()
        .view_access_key(
            sender,
            &key.public_key(),
            BlockReference::Finality(Finality::Final),
        )
        .await
        .expect("view sender access key");
    let delegate_v2 = DelegateActionV2 {
        sender_id: sender.clone(),
        receiver_id: receiver.clone(),
        actions: vec![
            NonDelegateAction::from_action(Action::transfer(amount))
                .expect("transfer is not a delegate"),
        ],
        // Ordinary full-access key → plain nonce (ak.nonce + 1).
        nonce: TransactionNonce::from_nonce(ak.nonce + 1),
        max_block_height,
        public_key: key.public_key(),
    };
    let payload: VersionedDelegateActionPayload = delegate_v2.into();
    let signature = key.sign(payload.get_hash().as_bytes());
    payload.into_signed(signature)
}

#[tokio::test]
async fn test_delegate_v2_relay() {
    let sandbox = sandbox_2_13().await;
    let root_near = sandbox.root_account();

    let (_, sender_id, sender_key) =
        funded_account(&root_near, &sandbox, "dv2sender", NearToken::from_near(10)).await;
    let (relayer_near, _, _) =
        funded_account(&root_near, &sandbox, "dv2relayer", NearToken::from_near(10)).await;
    let (_, recipient_id, _) = funded_account(
        &root_near,
        &sandbox,
        "dv2recipient",
        NearToken::from_near(1),
    )
    .await;

    let recipient_before = root_near
        .near()
        .balance(&recipient_id)
        .await
        .expect("recipient balance before")
        .total;

    let status = root_near.near().rpc().status().await.expect("status");
    let transfer_amount = NearToken::from_near(2);
    let signed = sign_delegate_v2(
        root_near.near(),
        &sender_key,
        &sender_id,
        &recipient_id,
        transfer_amount,
        status.sync_info.latest_block_height + 200,
    )
    .await;

    // The signature verifies under the V2 domain before we even send it.
    assert!(signed.verify(), "V2 delegate signature must verify locally");

    // Round-trip the payload the way a relayer would receive it (base64).
    let received = VersionedSignedDelegateAction::from_base64(&signed.to_base64())
        .expect("relayer decodes payload");
    assert_eq!(received, signed);

    // The outer receiver must be the meta-tx sender.
    relayer_near
        .wait_until::<Final>()
        .transaction(&sender_id)
        .add_action(Action::delegate_v2(received))
        .await
        .expect("relay DelegateV2")
        .result()
        .expect("DelegateV2 relay must succeed on-chain");

    let recipient_after = root_near
        .near()
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

    let status = root_near.near().rpc().status().await.expect("status");
    assert!(
        status.protocol_version >= REJECT_DELEGATE_V2_PROTOCOL_VERSION,
        "default sandbox runs protocol v{}, but DelegateV2 is only rejected from \
         v{REJECT_DELEGATE_V2_PROTOCOL_VERSION} (nearcore 2.14)",
        status.protocol_version,
    );

    let (_, sender_id, sender_key) =
        funded_account(&root_near, sandbox, "dv2rmsender", NearToken::from_near(10)).await;
    let (relayer_near, relayer_id, _) = funded_account(
        &root_near,
        sandbox,
        "dv2rmrelayer",
        NearToken::from_near(10),
    )
    .await;

    // A well-formed, correctly signed DelegateActionV2: the only reason for the
    // node to reject it is that the action itself was removed.
    let signed = sign_delegate_v2(
        root_near.near(),
        &sender_key,
        &sender_id,
        &relayer_id,
        NearToken::from_near(1),
        status.sync_info.latest_block_height + 200,
    )
    .await;

    let error = relayer_near
        .wait_until::<Final>()
        .transaction(&sender_id)
        .add_action(Action::delegate_v2(signed))
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
