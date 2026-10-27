//! Keys, signatures, and the [`Signer`] abstraction.
//!
//! Loading keys from the environment, a credentials file or the OS keyring
//! (`near_kit::signer::{from_env, from_credentials, from_keyring}`) lives in
//! `near-kit`; everything here does local cryptography only.

pub use crate::error::{ParseKeyError, SignerError};
pub use crate::types::{InMemorySigner, RotatingSigner, Signer, SigningBackend, SigningKey};
pub use crate::types::{
    KeyType, ML_DSA_65_HASH_LENGTH, ML_DSA_65_PUBLIC_KEY_LENGTH, ML_DSA_65_SECRET_KEY_LENGTH,
    ML_DSA_65_SEED_LENGTH, ML_DSA_65_SIGNATURE_LENGTH, PublicKey, PublicKeyHandle, SecretKey,
    Signature,
};
