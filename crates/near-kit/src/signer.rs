//! Transaction and message signing APIs.

pub use crate::client::{
    EnvSigner, InMemorySigner, RotatingSigner, Signer, SigningBackend, SigningKey,
};
pub use crate::error::{KeyStoreError, ParseKeyError, SignerError};
#[cfg(feature = "mnemonic")]
pub use crate::types::{
    DEFAULT_HD_PATH, DEFAULT_ML_DSA_65_WORD_COUNT, DEFAULT_WORD_COUNT, generate_seed_phrase,
};
pub use crate::types::{
    KeyType, ML_DSA_65_HASH_LENGTH, ML_DSA_65_PUBLIC_KEY_LENGTH, ML_DSA_65_SECRET_KEY_LENGTH,
    ML_DSA_65_SEED_LENGTH, ML_DSA_65_SIGNATURE_LENGTH, PublicKey, PublicKeyHandle, SecretKey,
    Signature,
};

#[cfg(feature = "file-signer")]
pub use crate::client::FileSigner;
#[cfg(feature = "keyring")]
pub use crate::client::KeyringSigner;
