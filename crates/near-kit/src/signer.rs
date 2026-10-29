//! Transaction and message signing APIs.
//!
//! Keys, [`Signer`], [`InMemorySigner`], and [`RotatingSigner`] are re-exported
//! from `near-kit-primitives`. [`from_env`], `from_credentials` (feature
//! `file-signer`), and `from_keyring` (feature `keyring`) load a key once
//! from the environment or the OS into an [`InMemorySigner`].
//!
//! # Example
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! use near_kit::protocol::Action;
//! use near_kit::signer::InMemorySigner;
//! use near_kit::{Near, NearToken, account};
//!
//! # async fn example() -> Result<(), near_kit::Error> {
//! let signer = InMemorySigner::new(
//!     account!("alice.testnet"),
//!     "ed25519:3D4YudUahN1nawWogh8pAKSj92sUNMdbZGjn7kERKzYoTy8tnFQuwoGUC51DowKqorvkr2pytJSnwuSbsNVfqygr".parse()?,
//! );
//!
//! let alice = Near::testnet().build().with_signer(signer);
//!
//! alice
//!     .transaction(account!("bob.testnet"))
//!     .add_action(Action::transfer(NearToken::from_near(1)))
//!     .await?;
//! # Ok(())
//! # }
//! ```

pub use crate::client::{from_env, from_env_vars};
pub use crate::error::KeyStoreError;
#[doc(inline)]
pub use near_kit_primitives::signer::{
    InMemorySigner, ParseKeyError, RotatingSigner, Signer, SignerError, SigningBackend, SigningKey,
};
#[doc(inline)]
pub use near_kit_primitives::signer::{
    KeyType, ML_DSA_65_HASH_LENGTH, ML_DSA_65_PUBLIC_KEY_LENGTH, ML_DSA_65_SECRET_KEY_LENGTH,
    ML_DSA_65_SEED_LENGTH, ML_DSA_65_SIGNATURE_LENGTH, PublicKey, PublicKeyHandle, SecretKey,
    Signature,
};

#[cfg(feature = "keyring")]
pub use crate::client::from_keyring;
#[cfg(feature = "file-signer")]
pub use crate::client::{from_credentials, from_credentials_file};
