//! NEAR account ID type with validation.

use std::fmt::{self, Display};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use crate::error::ParseAccountIdError;

/// A NEAR account identifier.
///
/// Valid account IDs:
/// - Named: "alice.near", "bob.testnet", "sub.account.near"
/// - Implicit (64 hex chars): "0123456789abcdef..."
/// - EVM implicit (0x + 40 hex chars): "0x1234..."
///
/// # Examples
///
/// ```
/// use near_kit::AccountId;
///
/// let named: AccountId = "alice.testnet".parse().unwrap();
/// assert!(named.is_named());
///
/// let implicit = "0".repeat(64).parse::<AccountId>().unwrap();
/// assert!(implicit.is_implicit());
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(String);

impl AccountId {
    /// Parse and validate an account ID.
    pub fn new(s: impl Into<String>) -> Result<Self, ParseAccountIdError> {
        let s = s.into();
        Self::validate(&s)?;
        Ok(Self(s))
    }

    /// Create without validation (for internal use / testing).
    #[doc(hidden)]
    pub fn new_unchecked(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    /// Validate an account ID string.
    fn validate(s: &str) -> Result<(), ParseAccountIdError> {
        if s.is_empty() {
            return Err(ParseAccountIdError::Empty);
        }

        if s.len() > 64 {
            return Err(ParseAccountIdError::TooLong(s.to_string()));
        }

        // Check for EVM implicit account (0x prefix)
        if s.starts_with("0x") {
            if s.len() != 42 {
                return Err(ParseAccountIdError::InvalidFormat(s.to_string()));
            }
            // Validate hex characters after 0x
            for c in s[2..].chars() {
                if !c.is_ascii_hexdigit() {
                    return Err(ParseAccountIdError::InvalidChar(s.to_string(), c));
                }
            }
            return Ok(());
        }

        // Check for implicit account (64 hex chars)
        if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(());
        }

        // Named account validation
        if s.len() < 2 {
            return Err(ParseAccountIdError::TooShort(s.to_string()));
        }

        // Check each character
        for c in s.chars() {
            if !matches!(c, 'a'..='z' | '0'..='9' | '_' | '-' | '.') {
                return Err(ParseAccountIdError::InvalidChar(s.to_string(), c));
            }
        }

        // Check for valid structure (no leading/trailing dots, no consecutive dots)
        if s.starts_with('.') || s.ends_with('.') || s.contains("..") {
            return Err(ParseAccountIdError::InvalidFormat(s.to_string()));
        }

        // Check for valid structure (no leading/trailing hyphens or underscores per segment)
        for part in s.split('.') {
            if part.is_empty() {
                return Err(ParseAccountIdError::InvalidFormat(s.to_string()));
            }
            if part.starts_with('-') || part.ends_with('-') {
                return Err(ParseAccountIdError::InvalidFormat(s.to_string()));
            }
            if part.starts_with('_') || part.ends_with('_') {
                return Err(ParseAccountIdError::InvalidFormat(s.to_string()));
            }
        }

        Ok(())
    }

    /// Check if this is an implicit account (64 hex chars).
    pub fn is_implicit(&self) -> bool {
        self.0.len() == 64 && self.0.chars().all(|c| c.is_ascii_hexdigit())
    }

    /// Check if this is an EVM implicit account (0x prefix).
    pub fn is_evm_implicit(&self) -> bool {
        self.0.starts_with("0x") && self.0.len() == 42
    }

    /// Check if this is a named account.
    pub fn is_named(&self) -> bool {
        !self.is_implicit() && !self.is_evm_implicit()
    }

    /// Check if this is a top-level account (no dots, like "near" or "testnet").
    pub fn is_top_level(&self) -> bool {
        self.is_named() && !self.0.contains('.')
    }

    /// Check if this is a subaccount of another account.
    pub fn is_sub_account_of(&self, parent: &AccountId) -> bool {
        if !self.is_named() || !parent.is_named() {
            return false;
        }
        self.0.ends_with(&format!(".{}", parent.0)) && self.0.len() > parent.0.len() + 1
    }

    /// Get the parent account (e.g., "sub.alice.near" → "alice.near").
    pub fn parent(&self) -> Option<AccountId> {
        if !self.is_named() {
            return None;
        }
        self.0
            .find('.')
            .map(|i| AccountId(self.0[i + 1..].to_string()))
    }

    /// Get as string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AccountId {
    type Err = ParseAccountIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<&str> for AccountId {
    type Error = ParseAccountIdError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl TryFrom<String> for AccountId {
    type Error = ParseAccountIdError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for AccountId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl BorshSerialize for AccountId {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(&self.0, writer)
    }
}

impl BorshDeserialize for AccountId {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let s = String::deserialize_reader(reader)?;
        Ok(Self(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_named_accounts() {
        assert!("alice.testnet".parse::<AccountId>().is_ok());
        assert!("bob.near".parse::<AccountId>().is_ok());
        assert!("sub.alice.testnet".parse::<AccountId>().is_ok());
        assert!("a1.b2.c3.testnet".parse::<AccountId>().is_ok());
        assert!("test_account.near".parse::<AccountId>().is_ok());
        assert!("test-account.near".parse::<AccountId>().is_ok());
    }

    #[test]
    fn test_valid_implicit_accounts() {
        let hex64 = "0".repeat(64);
        let account: AccountId = hex64.parse().unwrap();
        assert!(account.is_implicit());
        assert!(!account.is_named());
    }

    #[test]
    fn test_valid_evm_accounts() {
        let evm = format!("0x{}", "a".repeat(40));
        let account: AccountId = evm.parse().unwrap();
        assert!(account.is_evm_implicit());
        assert!(!account.is_named());
    }

    #[test]
    fn test_invalid_accounts() {
        assert!("".parse::<AccountId>().is_err());
        assert!("a".parse::<AccountId>().is_err()); // too short
        assert!("A.near".parse::<AccountId>().is_err()); // uppercase
        assert!(".alice.near".parse::<AccountId>().is_err()); // leading dot
        assert!("alice.near.".parse::<AccountId>().is_err()); // trailing dot
        assert!("alice..near".parse::<AccountId>().is_err()); // consecutive dots
        assert!("-alice.near".parse::<AccountId>().is_err()); // leading hyphen
    }

    #[test]
    fn test_parent() {
        let account: AccountId = "sub.alice.testnet".parse().unwrap();
        let parent = account.parent().unwrap();
        assert_eq!(parent.as_str(), "alice.testnet");

        let top: AccountId = "testnet".parse().unwrap();
        assert!(top.parent().is_none());
    }

    #[test]
    fn test_is_sub_account_of() {
        let sub: AccountId = "sub.alice.testnet".parse().unwrap();
        let parent: AccountId = "alice.testnet".parse().unwrap();
        let testnet: AccountId = "testnet".parse().unwrap();

        assert!(sub.is_sub_account_of(&parent));
        assert!(parent.is_sub_account_of(&testnet));
    }
}
