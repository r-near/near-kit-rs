//! NEAR account ID type — re-exported from [`near_account_id`] with near-kit extensions.

pub use near_account_id::AccountId;
pub use near_account_id::AccountIdRef;
pub use near_account_id::AccountType;
pub use near_account_id::TryIntoAccountId;

/// Extension trait adding near-kit ergonomic helpers to [`AccountId`].
pub trait AccountIdExt {
    /// Check if this is a NEAR-implicit account (64 hex chars).
    fn is_implicit(&self) -> bool;

    /// Check if this is an EVM implicit account (0x prefix + 40 hex chars).
    fn is_evm_implicit(&self) -> bool;

    /// Check if this is a named account (not implicit, not EVM implicit).
    fn is_named(&self) -> bool;
}

impl AccountIdExt for AccountId {
    fn is_implicit(&self) -> bool {
        self.get_account_type() == AccountType::NearImplicitAccount
    }

    fn is_evm_implicit(&self) -> bool {
        self.get_account_type() == AccountType::EthImplicitAccount
    }

    fn is_named(&self) -> bool {
        self.get_account_type() == AccountType::NamedAccount
    }
}

impl AccountIdExt for AccountIdRef {
    fn is_implicit(&self) -> bool {
        self.get_account_type() == AccountType::NearImplicitAccount
    }

    fn is_evm_implicit(&self) -> bool {
        self.get_account_type() == AccountType::EthImplicitAccount
    }

    fn is_named(&self) -> bool {
        self.get_account_type() == AccountType::NamedAccount
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

        // Top-level accounts have no parent in upstream's semantics
        // (upstream returns None for TLAs)
        let top: AccountId = "testnet".parse().unwrap();
        assert!(top.parent().is_none());
    }

    #[test]
    fn test_is_sub_account_of() {
        let sub: AccountId = "sub.alice.testnet".parse().unwrap();
        let parent: AccountId = "alice.testnet".parse().unwrap();

        assert!(sub.is_sub_account_of(&parent));
    }
}
