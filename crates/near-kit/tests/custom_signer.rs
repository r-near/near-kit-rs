use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use near_kit::AccountId;
use near_kit::signer::{
    PublicKey, SecretKey, Signature, Signer, SignerError, SigningBackend, SigningKey,
};

struct MockHsm {
    secret_key: SecretKey,
    signing_calls: Arc<AtomicUsize>,
}

impl SigningBackend for MockHsm {
    async fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        tokio::task::yield_now().await;
        self.signing_calls.fetch_add(1, Ordering::Relaxed);
        Ok(self.secret_key.sign(message))
    }
}

struct CustomSigner {
    account_id: AccountId,
    key: SigningKey,
}

impl Signer for CustomSigner {
    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn key(&self) -> SigningKey {
        self.key.clone()
    }

    fn public_key(&self) -> PublicKey {
        self.key.public_key().clone()
    }
}

#[tokio::test]
async fn downstream_can_implement_an_async_signing_backend() {
    let secret_key = SecretKey::generate_ed25519();
    let public_key = secret_key.public_key();
    let signing_calls = Arc::new(AtomicUsize::new(0));
    let signer = CustomSigner {
        account_id: "hsm-user.testnet".parse().unwrap(),
        key: SigningKey::from_backend(
            public_key.clone(),
            MockHsm {
                secret_key,
                signing_calls: signing_calls.clone(),
            },
        ),
    };

    assert_eq!(signer.public_key(), public_key);
    assert_eq!(signer.public_key(), public_key);
    assert_eq!(signing_calls.load(Ordering::Relaxed), 0);

    let message = b"sign through an external backend";
    let claimed_key = signer.key();
    let signature = claimed_key.sign(message).await.unwrap();

    assert!(signature.verify(message, &public_key));
    assert_eq!(signing_calls.load(Ordering::Relaxed), 1);
}
