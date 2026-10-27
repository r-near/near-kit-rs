//! Error types for the client-free parts of near-kit.
//!
//! Each error here converts into `near_kit::Error` with `?` (the `From`
//! impls live in `near-kit`, next to that type), so code that uses both crates
//! can mix them freely.

use thiserror::Error;

use crate::types::{AccountId, ActionError, CryptoHash, FinalExecutionStatus, InvalidTxError};

/// Error parsing an account ID.
///
/// This is a re-export of the upstream [`near_account_id::ParseAccountError`].
pub type ParseAccountIdError = near_account_id::ParseAccountError;

/// Error parsing a decimal token amount (near-kit's `FtAmount::parse`).
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseAmountError {
    #[error("Invalid amount format: '{0}'")]
    InvalidFormat(String),

    #[error("Invalid number in amount: '{0}'")]
    InvalidNumber(String),

    #[error("Amount overflow: value too large")]
    Overflow,
}

/// Error parsing a public or secret key.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseKeyError {
    #[error("Invalid key format: expected 'ed25519:...', 'secp256k1:...', or 'ml-dsa-65:...'")]
    InvalidFormat,

    #[error("Unknown key type: '{0}'")]
    UnknownKeyType(String),

    #[error("Invalid base58 encoding: {0}")]
    InvalidBase58(String),

    #[error("Invalid key length: expected {expected} bytes, got {actual}")]
    InvalidLength { expected: usize, actual: usize },

    #[error("Invalid curve point: key bytes do not represent a valid point on the curve")]
    InvalidCurvePoint,

    #[error("Invalid scalar: secret key bytes are not a valid scalar for this curve")]
    InvalidScalar,

    /// The string is an `ml-dsa-65-hash:` handle — the on-chain identifier
    /// (SHA3-256 digest) of an ML-DSA-65 access key that view RPCs return —
    /// not a public key. Parse it as a
    /// [`PublicKeyHandle`](crate::signer::PublicKeyHandle)
    /// instead; to sign or build a transaction, supply the full `ml-dsa-65:` key.
    #[error(
        "'ml-dsa-65-hash:' is the on-chain handle (hash) of an ML-DSA-65 key, not a public key; \
         supply the full 'ml-dsa-65:' key, or parse it as a PublicKeyHandle"
    )]
    MlDsa65HashHandle,
}

/// Error parsing a crypto hash.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ParseHashError {
    #[error("Invalid base58 encoding: {0}")]
    InvalidBase58(String),

    #[error("Invalid hash length: expected 32 bytes, got {0}")]
    InvalidLength(usize),
}

/// Error converting an RPC view back into a wire type
/// (`TryFrom<ActionView> for Action`).
///
/// View types are what the node *reports*; wire types are what gets signed and
/// sent. Most conversions are lossless, but a few views cannot be turned back
/// into an action, and those are surfaced here rather than silently producing
/// an action that would be rejected on the wire.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ActionViewConversionError {
    /// A base64-encoded field of the view failed to decode.
    #[error("Invalid base64 in `{field}`: {source}")]
    InvalidBase64 {
        /// Name of the view field that failed to decode.
        field: &'static str,
        /// The underlying base64 decode error.
        source: base64::DecodeError,
    },

    /// A `DeployContract` / `DeployGlobalContract*` view carried something
    /// other than a 32-byte code hash in its `code` field.
    ///
    /// The node replaces the WASM with the SHA-256 hash of the code in these
    /// views, so `code` must decode to exactly 32 bytes.
    #[error("Expected a 32-byte code hash in the deploy action view's `code`, got {actual} bytes")]
    InvalidCodeHash {
        /// Number of bytes `code` actually decoded to.
        actual: usize,
    },

    /// A deploy action view contains only the code hash, not the contract code
    /// required to construct a wire action.
    #[error(
        "Cannot convert deploy action view: the node returned only code hash {code_hash}, not the contract code"
    )]
    DeployCodeUnavailable {
        /// Hash of the contract code omitted from the view.
        code_hash: CryptoHash,
    },

    /// A `Delegate` / `DelegateV2` view contained a nested delegate action,
    /// which the protocol forbids.
    #[error("A delegate action must not contain a nested delegate action")]
    NestedDelegate,
}

/// Why bytes are not a valid universal account state init.
///
/// Mirrors nearcore's `StateInitError`. Returned by
/// [`UniversalStateInit::from_raw`](crate::protocol::UniversalStateInit::from_raw).
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum UniversalStateInitError {
    /// The bytes do not decode into a `UniversalStateInit` (including
    /// trailing bytes, an unknown version, or an invalid key tag).
    #[error("Bytes do not decode into a universal state init")]
    Malformed,
    /// The bytes decode, but are not the canonical borsh encoding of the
    /// result (unsorted or duplicate data keys or access keys). nearcore
    /// rejects these, since each value must have exactly one account id.
    #[error("Universal state init is not in canonical borsh form")]
    NonCanonical,
}

/// Error building a signer or signing with it.
///
/// Converts into `near_kit::Error` with `?`. A key that fails to parse
/// becomes `Error::ParseKey` and an invalid key set `Error::Config`.
#[derive(Debug, Clone, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum SignerError {
    #[error("Invalid seed phrase")]
    InvalidSeedPhrase,

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Key derivation failed: {0}")]
    KeyDerivationFailed(String),

    #[error("Implicit accounts require an Ed25519 secret key")]
    ImplicitAccountRequiresEd25519,

    /// A secret key did not parse.
    #[error(transparent)]
    InvalidKey(#[from] ParseKeyError),

    /// A rotating signer was given no keys.
    #[error("RotatingSigner requires at least one key")]
    NoKeys,

    /// A rotating signer was given signers for different accounts.
    #[error("All signers must share the same account ID, got {expected} and {found}")]
    MixedAccounts {
        expected: AccountId,
        found: AccountId,
    },
}

/// Error encoding contract arguments or decoding a contract's return value.
///
/// [`FunctionCall`](crate::transaction::FunctionCall) keeps the first
/// argument-encoding failure and returns it when the call is converted into
/// an action. Contract clients also return it, inside
/// [`ContractError::Codec`](crate::transaction::ContractError::Codec), when
/// a view result fails to decode. Converts into `near_kit::Error` with `?`
/// (`Error::Json` / `Error::Borsh`).
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CodecError {
    /// JSON serialization or deserialization failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Borsh serialization or deserialization failed.
    #[error("Borsh error: {0}")]
    Borsh(#[source] std::io::Error),
}

/// Lets infallible conversions (an [`Action`](crate::protocol::Action) into
/// itself) go wherever a fallible action conversion is accepted.
impl From<std::convert::Infallible> for CodecError {
    fn from(value: std::convert::Infallible) -> Self {
        match value {}
    }
}

/// Error decoding a [`SignedTransaction`](crate::protocol::SignedTransaction)
/// or [`SignedTransactionV1`](crate::protocol::SignedTransactionV1).
///
/// Converts into `near_kit::Error::InvalidTransaction` with `?`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DecodeTransactionError {
    /// The input is not valid base64.
    #[error("Invalid base64: {0}")]
    Base64(String),

    /// The bytes are not a Borsh-encoded signed transaction.
    #[error("Failed to deserialize signed transaction: {0}")]
    Borsh(#[source] std::io::Error),
}

/// Error reading the result of a transaction from its outcome.
///
/// Returned by
/// [`FinalExecutionOutcome::result`](crate::rpc::FinalExecutionOutcome::result)
/// and [`FinalExecutionOutcome::json`](crate::rpc::FinalExecutionOutcome::json),
/// and by the wait levels when the RPC response has no usable outcome.
/// Converts into `near_kit::Error` with `?`: [`InvalidTx`](Self::InvalidTx) and
/// [`Json`](Self::Json) keep their variants and the rest become
/// `Error::InvalidTransaction`.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OutcomeError {
    /// The transaction was rejected before execution.
    #[error("Invalid transaction: {0}")]
    InvalidTx(Box<InvalidTxError>),

    /// An action failed during execution.
    #[error("Action error: {0}")]
    ActionFailed(Box<ActionError>),

    /// The transaction has not finished with a return value.
    #[error("Transaction status is {0:?}, expected SuccessValue")]
    NotSuccess(Box<FinalExecutionStatus>),

    /// The return value is not valid base64.
    #[error("Failed to decode base64 SuccessValue: {0}")]
    InvalidBase64(String),

    /// The return value is not the expected JSON.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// The RPC response carried no execution outcome at an executed wait level.
    #[error(
        "RPC returned no execution outcome for transaction {transaction_hash} at wait level {wait_level}"
    )]
    MissingOutcome {
        transaction_hash: CryptoHash,
        wait_level: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ParseAccountIdError tests
    // ========================================================================

    #[test]
    fn test_parse_account_id_error_display() {
        // Upstream ParseAccountError has different variants.
        // Test via parsing invalid strings.
        let err = "".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("too short"));

        let err = "a".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("too short"));

        let err = "A.near".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("invalid character"));

        let err = "bad..account".parse::<AccountId>().unwrap_err();
        assert!(err.to_string().contains("redundant separator"));
    }

    // ========================================================================
    // ParseAmountError tests
    // ========================================================================

    #[test]
    fn test_parse_amount_error_display() {
        assert_eq!(
            ParseAmountError::InvalidFormat("xyz".to_string()).to_string(),
            "Invalid amount format: 'xyz'"
        );
        assert_eq!(
            ParseAmountError::InvalidNumber("abc".to_string()).to_string(),
            "Invalid number in amount: 'abc'"
        );
        assert_eq!(
            ParseAmountError::Overflow.to_string(),
            "Amount overflow: value too large"
        );
    }

    // ========================================================================
    // ParseKeyError tests
    // ========================================================================

    #[test]
    fn test_parse_key_error_display() {
        assert_eq!(
            ParseKeyError::InvalidFormat.to_string(),
            "Invalid key format: expected 'ed25519:...', 'secp256k1:...', or 'ml-dsa-65:...'"
        );
        assert_eq!(
            ParseKeyError::UnknownKeyType("rsa".to_string()).to_string(),
            "Unknown key type: 'rsa'"
        );
        assert_eq!(
            ParseKeyError::InvalidBase58("invalid chars".to_string()).to_string(),
            "Invalid base58 encoding: invalid chars"
        );
        assert_eq!(
            ParseKeyError::InvalidLength {
                expected: 32,
                actual: 16
            }
            .to_string(),
            "Invalid key length: expected 32 bytes, got 16"
        );
        assert_eq!(
            ParseKeyError::InvalidCurvePoint.to_string(),
            "Invalid curve point: key bytes do not represent a valid point on the curve"
        );
    }

    // ========================================================================
    // ParseHashError tests
    // ========================================================================

    #[test]
    fn test_parse_hash_error_display() {
        assert_eq!(
            ParseHashError::InvalidBase58("bad input".to_string()).to_string(),
            "Invalid base58 encoding: bad input"
        );
        assert_eq!(
            ParseHashError::InvalidLength(16).to_string(),
            "Invalid hash length: expected 32 bytes, got 16"
        );
    }

    // ========================================================================
    // SignerError tests
    // ========================================================================

    #[test]
    fn test_signer_error_display() {
        assert_eq!(
            SignerError::InvalidSeedPhrase.to_string(),
            "Invalid seed phrase"
        );
        assert_eq!(
            SignerError::SigningFailed("hardware failure".to_string()).to_string(),
            "Signing failed: hardware failure"
        );
        assert_eq!(
            SignerError::KeyDerivationFailed("path error".to_string()).to_string(),
            "Key derivation failed: path error"
        );
        assert_eq!(
            SignerError::ImplicitAccountRequiresEd25519.to_string(),
            "Implicit accounts require an Ed25519 secret key"
        );
    }
}
