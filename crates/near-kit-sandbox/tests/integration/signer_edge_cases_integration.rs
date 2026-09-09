//! Signer behaviors that need real access-key and nonce state.

use near_kit::Error;
use near_kit::NearToken;
use near_kit::rpc::RpcError;
use near_kit::signer::{InMemorySigner, RotatingSigner, SecretKey};
use near_kit::transaction::Final;

use super::support::{client_for, funded_account, shared_client, unique_account};

#[tokio::test]
async fn rotating_signer_uses_every_on_chain_key() {
    let (_, root) = shared_client().await;
    let keys = [
        SecretKey::generate_ed25519(),
        SecretKey::generate_ed25519(),
        SecretKey::generate_ed25519(),
    ];
    let account_id = unique_account("rotate");

    root.transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(100))
        .add_full_access_key(keys[0].public_key())
        .add_full_access_key(keys[1].public_key())
        .add_full_access_key(keys[2].public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("multi-key account");

    let signer = RotatingSigner::new(&account_id, keys.to_vec()).expect("rotating signer");
    let account = root.with_signer(signer);

    for index in 0..6 {
        let child = format!("child{index}.{account_id}");
        account
            .transaction(&child)
            .create_account()
            .transfer(NearToken::from_near(1))
            .add_full_access_key(SecretKey::generate_ed25519().public_key())
            .send()
            .wait_until::<Final>()
            .await
            .expect("rotating-key transaction");
        assert!(root.account_exists(&child).await.expect("child query"));
    }
}

#[tokio::test]
async fn sign_with_overrides_the_client_signer() {
    let (sandbox, root) = shared_client().await;
    let (default_client, _, _) =
        funded_account(&root, sandbox, "defaultsigner", NearToken::from_near(10)).await;
    let (_, override_id, override_key) =
        funded_account(&root, sandbox, "override", NearToken::from_near(10)).await;
    let child = format!("child.{override_id}");
    let override_signer =
        InMemorySigner::from_secret_key(override_id, override_key).expect("override signer");

    default_client
        .transaction(&child)
        .create_account()
        .transfer(NearToken::from_near(1))
        .add_full_access_key(SecretKey::generate_ed25519().public_key())
        .sign_with(override_signer)
        .send()
        .wait_until::<Final>()
        .await
        .expect("signer override transaction");

    assert!(root.account_exists(&child).await.expect("child query"));
}

#[tokio::test]
async fn deleted_key_is_rejected() {
    let (sandbox, root) = shared_client().await;
    let key_to_delete = SecretKey::generate_ed25519();
    let deleted_public_key = key_to_delete.public_key();
    let retained_key = SecretKey::generate_ed25519();
    let account_id = unique_account("deletedkey");

    root.transaction(&account_id)
        .create_account()
        .transfer(NearToken::from_near(10))
        .add_full_access_key(key_to_delete.public_key())
        .add_full_access_key(retained_key.public_key())
        .send()
        .wait_until::<Final>()
        .await
        .expect("two-key account");

    client_for(sandbox, &account_id, &retained_key)
        .delete_key(deleted_public_key.clone())
        .wait_until::<Final>()
        .await
        .expect("key deletion");

    let deleted_key_client = client_for(sandbox, &account_id, &key_to_delete);
    let error = deleted_key_client
        .transfer("sandbox", NearToken::from_near(1))
        .await
        .expect_err("deleted key must be rejected");
    match error {
        Error::Rpc(error) => match error.as_ref() {
            RpcError::AccessKeyNotFound {
                account_id: missing_account,
                public_key: missing_key,
                ..
            } => {
                assert_eq!(missing_account, &account_id);
                assert_eq!(missing_key, &deleted_public_key);
            }
            other => panic!("expected AccessKeyNotFound, got {other:?}"),
        },
        other => panic!("expected RPC access-key error, got {other:?}"),
    }
}
