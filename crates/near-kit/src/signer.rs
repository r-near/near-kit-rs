//! Transaction and message signing APIs.

pub use crate::client::{
    EnvSigner, InMemorySigner, RotatingSigner, Signer, SigningBackend, SigningKey,
};
pub use crate::error::{KeyStoreError, ParseKeyError, SignerError};
pub use crate::types::{
    DEFAULT_HD_PATH, DEFAULT_ML_DSA_65_WORD_COUNT, DEFAULT_WORD_COUNT, KeyPair, KeyType,
    ML_DSA_65_HASH_LENGTH, ML_DSA_65_PUBLIC_KEY_LENGTH, ML_DSA_65_SECRET_KEY_LENGTH,
    ML_DSA_65_SEED_LENGTH, ML_DSA_65_SIGNATURE_LENGTH, MlDsa65SecretKey, PublicKey,
    PublicKeyHandle, SecretKey, Signature, generate_seed_phrase,
};

#[cfg(feature = "file-signer")]
pub use crate::client::FileSigner;
#[cfg(feature = "keyring")]
pub use crate::client::KeyringSigner;
