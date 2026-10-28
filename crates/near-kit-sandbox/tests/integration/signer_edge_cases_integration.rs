//! Signer behaviors that need real access-key and nonce state.

use std::future::IntoFuture;

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, Finality, RpcError};
use near_kit::signer::{InMemorySigner, RotatingSigner, SecretKey};
use near_kit::transaction::Final;
use near_kit::{AccountId, Error};
use near_kit::{Near, NearToken};

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

    root.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(100)))
        .add_action(Action::add_full_access_key(keys[0].public_key()))
        .add_action(Action::add_full_access_key(keys[1].public_key()))
        .add_action(Action::add_full_access_key(keys[2].public_key()))
        .await
        .expect("multi-key account")
        .result()
        .unwrap();

    let signer = RotatingSigner::new(&account_id, keys.to_vec()).expect("rotating signer");
    let account = root.near().with_signer(signer);

    for index in 0..6 {
        let child: AccountId = format!("child{index}.{account_id}").parse().unwrap();
        account
            .wait_until::<Final>()
            .transaction(&child)
            .add_action(Action::create_account())
            .add_action(Action::transfer(NearToken::from_near(1)))
            .add_action(Action::add_full_access_key(
                SecretKey::generate_ed25519().public_key(),
            ))
            .await
            .expect("rotating-key transaction")
            .result()
            .unwrap();
        assert!(
            root.near()
                .account_exists(&child)
                .await
                .expect("child query")
        );
    }
}

#[tokio::test]
async fn with_signer_replaces_the_client_signer() {
    let (sandbox, root) = shared_client().await;
    let (default_client, _, _) =
        funded_account(&root, sandbox, "defaultsigner", NearToken::from_near(10)).await;
    let (_, override_id, override_key) =
        funded_account(&root, sandbox, "override", NearToken::from_near(10)).await;
    let child: AccountId = format!("child.{override_id}").parse().unwrap();
    let override_signer = InMemorySigner::new(override_id, override_key);

    default_client
        .near()
        .with_signer(override_signer)
        .wait_until::<Final>()
        .transaction(&child)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .add_action(Action::add_full_access_key(
            SecretKey::generate_ed25519().public_key(),
        ))
        .await
        .expect("signer override transaction")
        .result()
        .unwrap();

    assert!(
        root.near()
            .account_exists(&child)
            .await
            .expect("child query")
    );
}

#[tokio::test]
async fn deleted_key_is_rejected() {
    let (sandbox, root) = shared_client().await;
    let key_to_delete = SecretKey::generate_ed25519();
    let deleted_public_key = key_to_delete.public_key();
    let retained_key = SecretKey::generate_ed25519();
    let account_id = unique_account("deletedkey");

    root.wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(key_to_delete.public_key()))
        .add_action(Action::add_full_access_key(retained_key.public_key()))
        .await
        .expect("two-key account")
        .result()
        .unwrap();

    let retained_key_client = client_for(sandbox, &account_id, &retained_key);
    retained_key_client
        .wait_until::<Final>()
        .transaction(&account_id)
        .add_action(Action::delete_key(deleted_public_key.clone()))
        .await
        .expect("key deletion")
        .result()
        .unwrap();

    let deleted_key_client = client_for(sandbox, &account_id, &key_to_delete);
    let error = deleted_key_client
        .transaction(near_kit::account!("sandbox"))
        .add_action(Action::transfer(NearToken::from_near(1)))
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

/// The same endpoint spelled two ways (`localhost` / `127.0.0.1`) is one
/// chain: both clients draw from one nonce sequence, so identical concurrent
/// transfers are two transactions, not one signed twice.
#[tokio::test]
async fn clients_on_two_url_spellings_share_one_nonce_sequence() {
    let (sandbox, root) = shared_client().await;
    let (via_name, account_id, key) =
        funded_account(&root, sandbox, "urlspell", NearToken::from_near(5)).await;
    let (_, receiver, _) = funded_account(&root, sandbox, "urlrecv", NearToken::from_near(1)).await;

    let url = via_name.near().rpc().url().to_string();
    let other_url = if url.contains("localhost") {
        url.replacen("localhost", "127.0.0.1", 1)
    } else if url.contains("127.0.0.1") {
        url.replacen("127.0.0.1", "localhost", 1)
    } else {
        format!("{url}/")
    };
    assert_ne!(url, other_url);
    let via_ip = Near::custom(other_url, via_name.near().chain_id().clone())
        .build()
        .with_signer(InMemorySigner::new(account_id.clone(), key));

    // Signing alone already shows the shared sequence.
    let first = via_name
        .sign(
            &receiver,
            vec![Action::transfer(NearToken::from_yoctonear(1))],
        )
        .await
        .expect("sign via the first spelling");
    let second = via_ip
        .sign(
            &receiver,
            vec![Action::transfer(NearToken::from_yoctonear(1))],
        )
        .await
        .expect("sign via the second spelling");
    assert_eq!(second.transaction.nonce, first.transaction.nonce + 1);

    // Identical transfers through both at once both land.
    let before = root
        .near()
        .rpc()
        .view_account(&receiver, BlockReference::Finality(Finality::Final))
        .await
        .expect("receiver balance")
        .amount;
    let (a, b) = tokio::join!(
        via_name
            .transaction(&receiver)
            .add_action(Action::transfer(NearToken::from_yoctonear(1)))
            .into_future(),
        via_ip
            .transaction(&receiver)
            .add_action(Action::transfer(NearToken::from_yoctonear(1)))
            .into_future(),
    );
    let (a, b) = (a.expect("first transfer"), b.expect("second transfer"));
    assert_ne!(a.transaction_hash(), b.transaction_hash());
    let after = root
        .near()
        .rpc()
        .view_account(&receiver, BlockReference::Finality(Finality::Final))
        .await
        .expect("receiver balance")
        .amount;
    assert_eq!(
        after.as_yoctonear() - before.as_yoctonear(),
        2,
        "both transfers ran"
    );
}
