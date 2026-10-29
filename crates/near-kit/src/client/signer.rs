//! Load an [`InMemorySigner`] from the environment or a near-cli credentials file.

#[cfg(feature = "file-signer")]
use std::path::Path;

use crate::error::Error;
use crate::types::{AccountId, InMemorySigner};

/// Load a signer from the `NEAR_ACCOUNT_ID` and `NEAR_PRIVATE_KEY`
/// environment variables.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::signer;
///
/// // With NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY set:
/// let signer = signer::from_env()?;
/// # Ok::<(), near_kit::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::Config`] if either variable is missing, and a parse error if
/// either value is invalid.
pub fn from_env() -> Result<InMemorySigner, Error> {
    from_env_vars("NEAR_ACCOUNT_ID", "NEAR_PRIVATE_KEY")
}

/// Load a signer from the environment variables named `account_var` (the
/// account ID) and `key_var` (its private key, e.g. `"ed25519:..."`).
///
/// # Errors
///
/// [`Error::Config`] if either variable is missing, and a parse error if
/// either value is invalid.
pub fn from_env_vars(account_var: &str, key_var: &str) -> Result<InMemorySigner, Error> {
    match (std::env::var(account_var).ok(), std::env::var(key_var).ok()) {
        (Some(account_id), Some(key)) => Ok(InMemorySigner::new(
            account_id.parse::<AccountId>()?,
            key.parse()?,
        )),
        (Some(_), None) => Err(Error::Config(format!(
            "{account_var} is set but {key_var} is missing"
        ))),
        (None, Some(_)) => Err(Error::Config(format!(
            "{key_var} is set but {account_var} is missing"
        ))),
        (None, None) => Err(Error::Config(format!(
            "{account_var} and {key_var} must be set"
        ))),
    }
}

/// Credential file format compatible with near-cli.
#[cfg(feature = "file-signer")]
#[derive(serde::Deserialize)]
struct CredentialFile {
    #[serde(alias = "secret_key")]
    private_key: String,
    #[serde(default)]
    public_key: Option<String>,
}

/// Load the key for `account_id` from
/// `~/.near-credentials/{network}/{account_id}.json`, the file near-cli and
/// near-cli-rs write.
///
/// Requires the opt-in `file-signer` feature and is not usable on
/// `wasm32-unknown-unknown`, which has no filesystem.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::{account, signer};
///
/// // Load from ~/.near-credentials/testnet/alice.testnet.json
/// let signer = signer::from_credentials("testnet", account!("alice.testnet"))?;
/// # Ok::<(), near_kit::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::Config`] if the home directory can't be determined or the file
/// can't be read or parsed, and a parse error for an invalid key.
#[cfg(feature = "file-signer")]
pub fn from_credentials(
    network: &str,
    account_id: impl Into<AccountId>,
) -> Result<InMemorySigner, Error> {
    let account_id: AccountId = account_id.into();
    let home = dirs::home_dir()
        .ok_or_else(|| Error::Config("Could not determine home directory".to_string()))?;
    let path = home
        .join(".near-credentials")
        .join(network)
        .join(format!("{account_id}.json"));
    from_credentials_file(&path, account_id)
}

/// Load the key for `account_id` from a near-cli credentials file at `path`.
///
/// Requires the opt-in `file-signer` feature. To sign with several keys of
/// one account, load each and combine them:
///
/// ```rust,no_run
/// use near_kit::account;
/// use near_kit::signer::{self, RotatingSigner};
///
/// let rotating = RotatingSigner::from_signers(vec![
///     signer::from_credentials_file("keys/bot-key-0.json", account!("bot.testnet"))?,
///     signer::from_credentials_file("keys/bot-key-1.json", account!("bot.testnet"))?,
/// ])?;
/// # Ok::<(), near_kit::Error>(())
/// ```
///
/// # Errors
///
/// [`Error::Config`] if the file can't be read or parsed, or its
/// `public_key` names a different key than `private_key`, and a parse error
/// for an invalid key.
#[cfg(feature = "file-signer")]
pub fn from_credentials_file(
    path: impl AsRef<Path>,
    account_id: impl Into<AccountId>,
) -> Result<InMemorySigner, Error> {
    let path = path.as_ref();
    let content = std::fs::read_to_string(path).map_err(|e| {
        Error::Config(format!(
            "Failed to read credentials file {}: {}",
            path.display(),
            e
        ))
    })?;
    let credentials: CredentialFile = serde_json::from_str(&content).map_err(|e| {
        Error::Config(format!(
            "Failed to parse credentials file {}: {}",
            path.display(),
            e
        ))
    })?;
    let secret_key: crate::types::SecretKey = credentials.private_key.parse()?;
    // The file's `public_key` (a full key, or an ML-DSA-65 hash handle) must
    // name the key `private_key` derives. A value near-kit can't parse is
    // left unchecked.
    if let Some(stored) = credentials
        .public_key
        .as_deref()
        .and_then(|s| s.parse::<crate::types::PublicKeyHandle>().ok())
        && !stored.refers_to(&secret_key.public_key())
    {
        return Err(Error::Config(format!(
            "Credentials file {}: public_key {} does not match the private key's {}",
            path.display(),
            stored,
            secret_key.public_key()
        )));
    }
    Ok(InMemorySigner::new(account_id, secret_key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_vars_must_both_be_set() {
        let error = from_env_vars("NONEXISTENT_VAR_1", "NONEXISTENT_VAR_2").unwrap_err();
        assert_eq!(
            error.to_string(),
            "Invalid configuration: NONEXISTENT_VAR_1 and NONEXISTENT_VAR_2 must be set"
        );
    }

    #[cfg(feature = "file-signer")]
    #[test]
    fn credentials_files_load_a_plain_in_memory_signer() {
        use crate::signer::{SecretKey, Signer};

        let key = SecretKey::ed25519_from_bytes([20; 32]);
        let contents = serde_json::json!({
            "account_id": "alice.testnet",
            "private_key": key.to_string(),
        });
        let signer = load_credentials("plain", &contents.to_string()).unwrap();
        assert_eq!(signer.account_id().as_str(), "alice.testnet");
        assert_eq!(signer.public_key(), key.public_key());
    }

    /// Write `contents` to a fresh temp file, load it for `alice.testnet`,
    /// and remove the file.
    #[cfg(feature = "file-signer")]
    fn load_credentials(name: &str, contents: &str) -> Result<InMemorySigner, Error> {
        let path = std::env::temp_dir().join(format!(
            "near-kit-credentials-{name}-{}.json",
            std::process::id()
        ));
        std::fs::write(&path, contents).unwrap();
        let result = from_credentials_file(&path, crate::account!("alice.testnet"));
        std::fs::remove_file(&path).unwrap();
        result
    }

    /// near-cli-rs writes ML-DSA-65 keys in nearcore's 4032-byte expanded form.
    #[cfg(feature = "file-signer")]
    #[test]
    fn credentials_file_with_expanded_ml_dsa65_key_loads() {
        use crate::signer::{SecretKey, Signer};

        let key = SecretKey::ml_dsa65_from_seed([21; 32]);
        let expanded = key.to_ml_dsa65_expanded_bytes().unwrap();
        let contents = serde_json::json!({
            "account_id": "alice.testnet",
            "public_key": key.public_key().to_string(),
            "private_key": format!("ml-dsa-65:{}", bs58::encode(expanded.as_slice()).into_string()),
        });

        let signer = load_credentials("ml-dsa-65", &contents.to_string()).unwrap();
        assert_eq!(signer.public_key(), key.public_key());
    }

    /// near-api-js writes secp256k1 keys as `secret (32) || public key (64)`.
    #[cfg(feature = "file-signer")]
    #[test]
    fn credentials_file_with_near_api_js_secp256k1_key_loads() {
        use crate::signer::{SecretKey, Signer};

        let key = SecretKey::secp256k1_from_bytes([22; 32]).unwrap();
        let mut keypair = key.as_bytes().to_vec();
        keypair.extend_from_slice(key.public_key().as_bytes());
        let contents = serde_json::json!({
            "account_id": "alice.testnet",
            "public_key": key.public_key().to_string(),
            "private_key": format!("secp256k1:{}", bs58::encode(&keypair).into_string()),
        });

        let signer = load_credentials("secp256k1", &contents.to_string()).unwrap();
        assert_eq!(signer.public_key(), key.public_key());
    }

    #[cfg(feature = "file-signer")]
    #[test]
    fn credentials_file_whose_public_key_does_not_match_is_rejected() {
        use crate::signer::SecretKey;

        let key = SecretKey::ed25519_from_bytes([23; 32]);
        let other = SecretKey::ed25519_from_bytes([24; 32]);
        let contents = serde_json::json!({
            "account_id": "alice.testnet",
            "public_key": other.public_key().to_string(),
            "private_key": key.to_string(),
        });
        let error = load_credentials("mismatch", &contents.to_string()).unwrap_err();
        assert!(
            error.to_string().contains("does not match"),
            "unexpected error: {error}"
        );

        // The ML-DSA-65 hash handle of the right key is accepted.
        let key = SecretKey::ml_dsa65_from_seed([25; 32]);
        let contents = serde_json::json!({
            "public_key": key.public_key().to_ml_dsa65_hash().unwrap().to_string(),
            "private_key": key.to_string(),
        });
        assert!(load_credentials("handle", &contents.to_string()).is_ok());
    }
}
