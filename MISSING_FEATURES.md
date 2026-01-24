# Missing Features in near-kit-rs

This document catalogs features missing from `near-kit-rs` compared to `near-kit` (TypeScript) and `near-api-rs`. Use this as a reference for implementation tasks.

---

## Table of Contents

1. [Seed Phrase / BIP39 Support](#1-seed-phrase--bip39-support)
2. [Fungible Token Helpers (NEP-141)](#2-fungible-token-helpers-nep-141)
3. [Non-Fungible Token Helpers (NEP-171)](#3-non-fungible-token-helpers-nep-171)
4. [Storage Deposit Management (NEP-145)](#4-storage-deposit-management-nep-145)
5. [Staking Pool Operations](#5-staking-pool-operations)
6. [Keystore / Keychain Signer](#6-keystore--keychain-signer)
7. [Ledger Hardware Wallet Support](#7-ledger-hardware-wallet-support)
8. [Gas Keys (NEP-611)](#8-gas-keys-nep-611)
9. [Transaction V1 with Priority Fee](#9-transaction-v1-with-priority-fee)
10. [Contract ABI Support](#10-contract-abi-support)
11. [View State / Storage Queries](#11-view-state--storage-queries)
12. [Presigning / Offline Signing](#12-presigning--offline-signing)
13. [Environment-Based Configuration](#13-environment-based-configuration)
14. [Contract Source Metadata (NEP-330)](#14-contract-source-metadata-nep-330)
15. [FT Amount Type with Decimal Handling](#15-ft-amount-type-with-decimal-handling)

---

## 1. Seed Phrase / BIP39 Support

### Priority: 🔴 Critical

### Description
Support for BIP39 mnemonic seed phrases with SLIP-0010 HD key derivation. This is essential for wallet compatibility and key recovery.

### Reference Implementations

**near-kit (TypeScript)** - `src/utils/key.ts`:
```typescript
function generateSeedPhrase(wordCount = 12): string
function parseSeedPhrase(phrase: string, path = "m/44'/397'/0'"): KeyPair
```

**near-api-rs** - `api/src/signer/mod.rs`:
```rust
pub fn generate_seed_phrase() -> Result<(String, PublicKey), SecretError>
pub fn generate_seed_phrase_with_word_count(count: usize) -> Result<(String, PublicKey), SecretError>
pub fn generate_seed_phrase_with_passphrase(passphrase: &str) -> Result<(String, PublicKey), SecretError>
pub fn get_secret_key_from_seed(hd_path: &str, phrase: &str, password: &str) -> Result<SecretKey, SecretError>

impl Signer {
    pub fn from_seed_phrase(phrase: &str, password: Option<&str>) -> Result<Self, SignerError>
    pub fn from_seed_phrase_with_hd_path(phrase: &str, password: Option<&str>, hd_path: &str) -> Result<Self, SignerError>
}
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/types/key.rs` (extend existing)

```rust
use bip39::{Mnemonic, Language};

/// Default NEAR HD derivation path (SLIP-0044 coin type 397)
pub const DEFAULT_HD_PATH: &str = "m/44'/397'/0'";

impl SecretKey {
    /// Generate a new random seed phrase (12 words)
    pub fn generate_seed_phrase() -> String {
        Self::generate_seed_phrase_with_word_count(12)
    }
    
    /// Generate a seed phrase with specified word count (12 or 24)
    pub fn generate_seed_phrase_with_word_count(word_count: usize) -> String {
        // Use bip39 crate
    }
    
    /// Derive a secret key from a seed phrase using default HD path
    pub fn from_seed_phrase(phrase: &str) -> Result<Self, ParseKeyError> {
        Self::from_seed_phrase_with_path(phrase, None, DEFAULT_HD_PATH)
    }
    
    /// Derive a secret key from a seed phrase with optional password and custom HD path
    pub fn from_seed_phrase_with_path(
        phrase: &str,
        password: Option<&str>,
        hd_path: &str,
    ) -> Result<Self, ParseKeyError> {
        // 1. Parse mnemonic with bip39
        // 2. Derive seed with optional password
        // 3. Use SLIP-0010 to derive ed25519 key at path
        // 4. Return SecretKey::Ed25519(...)
    }
}
```

**File**: `crates/near-kit/src/client/signer.rs` (extend existing)

```rust
/// Signer that derives keys from a seed phrase
pub struct SeedPhraseSigner {
    account_id: AccountId,
    secret_key: SecretKey,
    public_key: PublicKey,
}

impl SeedPhraseSigner {
    pub fn new(
        account_id: impl TryInto<AccountId>,
        phrase: &str,
    ) -> Result<Self, Error> {
        Self::with_path(account_id, phrase, None, DEFAULT_HD_PATH)
    }
    
    pub fn with_path(
        account_id: impl TryInto<AccountId>,
        phrase: &str,
        password: Option<&str>,
        hd_path: &str,
    ) -> Result<Self, Error> {
        let secret_key = SecretKey::from_seed_phrase_with_path(phrase, password, hd_path)?;
        let public_key = secret_key.public_key();
        Ok(Self {
            account_id: account_id.try_into()?,
            secret_key,
            public_key,
        })
    }
}

impl Signer for SeedPhraseSigner {
    // ... implement trait
}
```

### Dependencies to Add

```toml
[dependencies]
bip39 = "2.0"
slip10_ed25519 = "0.1"  # Or implement SLIP-0010 manually
```

### Tests to Write

```rust
#[test]
fn test_generate_seed_phrase_12_words() {
    let phrase = SecretKey::generate_seed_phrase();
    assert_eq!(phrase.split_whitespace().count(), 12);
}

#[test]
fn test_generate_seed_phrase_24_words() {
    let phrase = SecretKey::generate_seed_phrase_with_word_count(24);
    assert_eq!(phrase.split_whitespace().count(), 24);
}

#[test]
fn test_derive_from_seed_phrase() {
    // Known test vector from NEAR wallet
    let phrase = "word1 word2 ... word12";
    let expected_public_key = "ed25519:...";
    
    let secret = SecretKey::from_seed_phrase(phrase).unwrap();
    assert_eq!(secret.public_key().to_string(), expected_public_key);
}

#[test]
fn test_seed_phrase_with_password() {
    let phrase = "...";
    let key_without_pass = SecretKey::from_seed_phrase(phrase).unwrap();
    let key_with_pass = SecretKey::from_seed_phrase_with_path(phrase, Some("password"), DEFAULT_HD_PATH).unwrap();
    
    // Different passwords should produce different keys
    assert_ne!(key_without_pass.public_key(), key_with_pass.public_key());
}
```

---

## 2. Fungible Token Helpers (NEP-141)

### Priority: 🔴 Critical

### Description
High-level API for interacting with NEP-141 fungible token contracts. This is one of the most common use cases for NEAR developers.

### Reference Implementation

**near-api-rs** - `api/src/tokens.rs`:
```rust
// Query metadata
Tokens::ft_metadata(contract_id).fetch_from_mainnet().await?;

// Query balance
Tokens::account(account_id).ft_balance(ft_contract).fetch_from_mainnet().await?;

// Transfer
Tokens::account(sender_id)
    .send_to(receiver_id)
    .ft(ft_contract, amount)
    .with_signer(signer)
    .send_to_mainnet()
    .await?;

// Transfer with call (ft_transfer_call)
Tokens::account(sender_id)
    .send_to(receiver_contract)
    .ft_call(ft_contract, amount, msg)
    .with_signer(signer)
    .send_to_mainnet()
    .await?;
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/tokens/mod.rs` (new)

```rust
mod ft;
mod nft;

pub use ft::*;
pub use nft::*;
```

**File**: `crates/near-kit/src/tokens/ft.rs` (new)

```rust
use crate::{AccountId, Near, NearToken, Gas, Error};
use serde::{Deserialize, Serialize};

/// NEP-141 Fungible Token metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
    pub decimals: u8,
}

/// Fungible token client for a specific contract
pub struct FungibleToken<'a> {
    near: &'a Near,
    contract_id: AccountId,
}

impl<'a> FungibleToken<'a> {
    pub fn new(near: &'a Near, contract_id: AccountId) -> Self {
        Self { near, contract_id }
    }
    
    /// Get token metadata (ft_metadata)
    pub async fn metadata(&self) -> Result<FtMetadata, Error> {
        self.near
            .view::<FtMetadata>(&self.contract_id, "ft_metadata")
            .await
    }
    
    /// Get token balance for an account (ft_balance_of)
    pub async fn balance_of(&self, account_id: impl AsRef<str>) -> Result<u128, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        let balance: String = self.near
            .view(&self.contract_id, "ft_balance_of")
            .args(Args { account_id: account_id.as_ref() })
            .await?;
        
        balance.parse().map_err(|_| Error::ParseAmount(...))
    }
    
    /// Get total supply (ft_total_supply)
    pub async fn total_supply(&self) -> Result<u128, Error> {
        let supply: String = self.near
            .view(&self.contract_id, "ft_total_supply")
            .await?;
        supply.parse().map_err(|_| Error::ParseAmount(...))
    }
    
    /// Transfer tokens to a receiver (ft_transfer)
    /// Requires 1 yoctoNEAR deposit for security
    pub fn transfer(
        &self,
        receiver_id: impl AsRef<str>,
        amount: u128,
    ) -> FtTransferCall<'a> {
        FtTransferCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            receiver_id: receiver_id.as_ref().to_string(),
            amount,
            memo: None,
            msg: None,
            gas: Gas::tgas(30),
        }
    }
    
    /// Transfer tokens with a function call on receiver (ft_transfer_call)
    pub fn transfer_call(
        &self,
        receiver_id: impl AsRef<str>,
        amount: u128,
        msg: impl Into<String>,
    ) -> FtTransferCall<'a> {
        FtTransferCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            receiver_id: receiver_id.as_ref().to_string(),
            amount,
            memo: None,
            msg: Some(msg.into()),
            gas: Gas::tgas(50), // More gas for cross-contract call
        }
    }
}

/// Builder for FT transfer transactions
pub struct FtTransferCall<'a> {
    near: &'a Near,
    contract_id: AccountId,
    receiver_id: String,
    amount: u128,
    memo: Option<String>,
    msg: Option<String>,
    gas: Gas,
}

impl<'a> FtTransferCall<'a> {
    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }
    
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        self.gas = gas.into_gas().unwrap_or(Gas::tgas(30));
        self
    }
}

impl<'a> IntoFuture for FtTransferCall<'a> {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;
    
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let method = if self.msg.is_some() { "ft_transfer_call" } else { "ft_transfer" };
            
            #[derive(Serialize)]
            struct TransferArgs {
                receiver_id: String,
                amount: String,
                #[serde(skip_serializing_if = "Option::is_none")]
                memo: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                msg: Option<String>,
            }
            
            self.near
                .call(&self.contract_id, method)
                .args(TransferArgs {
                    receiver_id: self.receiver_id,
                    amount: self.amount.to_string(),
                    memo: self.memo,
                    msg: self.msg,
                })
                .gas(self.gas)
                .deposit(NearToken::yocto(1)) // Required 1 yocto deposit
                .await
        })
    }
}

// Add to Near client
impl Near {
    /// Get a fungible token client for a contract
    pub fn ft(&self, contract_id: impl TryInto<AccountId>) -> Result<FungibleToken<'_>, Error> {
        Ok(FungibleToken::new(self, contract_id.try_into()?))
    }
}
```

### Storage Registration Helper

FT transfers require the receiver to be registered. Add a helper:

```rust
impl<'a> FungibleToken<'a> {
    /// Check if an account is registered for this token
    pub async fn is_registered(&self, account_id: impl AsRef<str>) -> Result<bool, Error> {
        let balance = self.storage_balance_of(account_id).await?;
        Ok(balance.is_some())
    }
    
    /// Get storage balance for an account
    pub async fn storage_balance_of(
        &self,
        account_id: impl AsRef<str>,
    ) -> Result<Option<StorageBalance>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        self.near
            .view(&self.contract_id, "storage_balance_of")
            .args(Args { account_id: account_id.as_ref() })
            .await
    }
    
    /// Register an account for this token (storage_deposit)
    pub fn storage_deposit(
        &self,
        account_id: impl AsRef<str>,
    ) -> StorageDepositCall<'a> {
        StorageDepositCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            account_id: Some(account_id.as_ref().to_string()),
            deposit: NearToken::millinear(100), // Standard minimum
            registration_only: true,
        }
    }
}
```

### Tests to Write

```rust
#[tokio::test]
async fn test_ft_metadata() {
    let sandbox = SandboxConfig::shared().await;
    // Deploy a test FT contract
    // Query metadata
    // Assert fields
}

#[tokio::test]
async fn test_ft_transfer() {
    let sandbox = SandboxConfig::shared().await;
    // Deploy FT, mint tokens to sender
    // Register receiver
    // Transfer
    // Check balances
}

#[tokio::test]
async fn test_ft_transfer_call() {
    // Test cross-contract transfer
}
```

---

## 3. Non-Fungible Token Helpers (NEP-171)

### Priority: 🔴 Critical

### Description
High-level API for interacting with NEP-171 NFT contracts.

### Reference Implementation

**near-api-rs** - `api/src/tokens.rs`:
```rust
Tokens::nft_metadata(contract_id).fetch().await?;
Tokens::account(owner_id).nft_assets(nft_contract).fetch().await?;
Tokens::account(sender).send_to(receiver).nft(contract, token_id).send().await?;
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/tokens/nft.rs` (new)

```rust
use crate::{AccountId, Near, NearToken, Gas, Error};
use serde::{Deserialize, Serialize};

/// NEP-171 NFT Contract metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftContractMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub icon: Option<String>,
    pub base_uri: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

/// NEP-171 Token metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftTokenMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub media: Option<String>,
    pub media_hash: Option<String>,
    pub copies: Option<u64>,
    pub issued_at: Option<String>,
    pub expires_at: Option<String>,
    pub starts_at: Option<String>,
    pub updated_at: Option<String>,
    pub extra: Option<String>,
    pub reference: Option<String>,
    pub reference_hash: Option<String>,
}

/// NFT Token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftToken {
    pub token_id: String,
    pub owner_id: String,
    pub metadata: Option<NftTokenMetadata>,
    pub approved_account_ids: Option<std::collections::HashMap<String, u64>>,
}

/// Non-fungible token client for a specific contract
pub struct NonFungibleToken<'a> {
    near: &'a Near,
    contract_id: AccountId,
}

impl<'a> NonFungibleToken<'a> {
    pub fn new(near: &'a Near, contract_id: AccountId) -> Self {
        Self { near, contract_id }
    }
    
    /// Get contract metadata (nft_metadata)
    pub async fn metadata(&self) -> Result<NftContractMetadata, Error> {
        self.near
            .view(&self.contract_id, "nft_metadata")
            .await
    }
    
    /// Get a specific token (nft_token)
    pub async fn token(&self, token_id: impl AsRef<str>) -> Result<Option<NftToken>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            token_id: &'a str,
        }
        
        self.near
            .view(&self.contract_id, "nft_token")
            .args(Args { token_id: token_id.as_ref() })
            .await
    }
    
    /// Get tokens owned by an account (nft_tokens_for_owner)
    pub async fn tokens_for_owner(
        &self,
        account_id: impl AsRef<str>,
        from_index: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Vec<NftToken>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            from_index: Option<String>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<u64>,
        }
        
        self.near
            .view(&self.contract_id, "nft_tokens_for_owner")
            .args(Args {
                account_id: account_id.as_ref(),
                from_index: from_index.map(|i| i.to_string()),
                limit,
            })
            .await
    }
    
    /// Get total supply (nft_total_supply)
    pub async fn total_supply(&self) -> Result<u64, Error> {
        let supply: String = self.near
            .view(&self.contract_id, "nft_total_supply")
            .await?;
        supply.parse().map_err(|_| Error::ParseAmount(...))
    }
    
    /// Get supply for an owner (nft_supply_for_owner)
    pub async fn supply_for_owner(&self, account_id: impl AsRef<str>) -> Result<u64, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        let supply: String = self.near
            .view(&self.contract_id, "nft_supply_for_owner")
            .args(Args { account_id: account_id.as_ref() })
            .await?;
        supply.parse().map_err(|_| Error::ParseAmount(...))
    }
    
    /// Transfer an NFT (nft_transfer)
    /// Requires 1 yoctoNEAR deposit
    pub fn transfer(
        &self,
        receiver_id: impl AsRef<str>,
        token_id: impl AsRef<str>,
    ) -> NftTransferCall<'a> {
        NftTransferCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            receiver_id: receiver_id.as_ref().to_string(),
            token_id: token_id.as_ref().to_string(),
            approval_id: None,
            memo: None,
            msg: None,
            gas: Gas::tgas(30),
        }
    }
    
    /// Transfer an NFT with function call on receiver (nft_transfer_call)
    pub fn transfer_call(
        &self,
        receiver_id: impl AsRef<str>,
        token_id: impl AsRef<str>,
        msg: impl Into<String>,
    ) -> NftTransferCall<'a> {
        NftTransferCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            receiver_id: receiver_id.as_ref().to_string(),
            token_id: token_id.as_ref().to_string(),
            approval_id: None,
            memo: None,
            msg: Some(msg.into()),
            gas: Gas::tgas(50),
        }
    }
}

/// Builder for NFT transfer transactions
pub struct NftTransferCall<'a> {
    near: &'a Near,
    contract_id: AccountId,
    receiver_id: String,
    token_id: String,
    approval_id: Option<u64>,
    memo: Option<String>,
    msg: Option<String>,
    gas: Gas,
}

impl<'a> NftTransferCall<'a> {
    pub fn approval_id(mut self, id: u64) -> Self {
        self.approval_id = Some(id);
        self
    }
    
    pub fn memo(mut self, memo: impl Into<String>) -> Self {
        self.memo = Some(memo.into());
        self
    }
    
    pub fn gas(mut self, gas: impl IntoGas) -> Self {
        self.gas = gas.into_gas().unwrap_or(Gas::tgas(30));
        self
    }
}

impl<'a> IntoFuture for NftTransferCall<'a> {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;
    
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let method = if self.msg.is_some() { "nft_transfer_call" } else { "nft_transfer" };
            
            #[derive(Serialize)]
            struct TransferArgs {
                receiver_id: String,
                token_id: String,
                #[serde(skip_serializing_if = "Option::is_none")]
                approval_id: Option<u64>,
                #[serde(skip_serializing_if = "Option::is_none")]
                memo: Option<String>,
                #[serde(skip_serializing_if = "Option::is_none")]
                msg: Option<String>,
            }
            
            self.near
                .call(&self.contract_id, method)
                .args(TransferArgs {
                    receiver_id: self.receiver_id,
                    token_id: self.token_id,
                    approval_id: self.approval_id,
                    memo: self.memo,
                    msg: self.msg,
                })
                .gas(self.gas)
                .deposit(NearToken::yocto(1))
                .await
        })
    }
}

// Add to Near client
impl Near {
    /// Get an NFT client for a contract
    pub fn nft(&self, contract_id: impl TryInto<AccountId>) -> Result<NonFungibleToken<'_>, Error> {
        Ok(NonFungibleToken::new(self, contract_id.try_into()?))
    }
}
```

---

## 4. Storage Deposit Management (NEP-145)

### Priority: 🟡 Important

### Description
NEP-145 defines a standard for storage staking on contracts. This is required before transferring FTs to unregistered accounts.

### Reference Implementation

**near-api-rs** - `api/src/storage.rs`:
```rust
StorageDeposit::on_contract(contract_id)
    .view_account_storage(account_id).fetch().await?;
    .deposit(account_id, amount).send().await?;
    .withdraw(account_id, amount).send().await?;
    .unregister().force().send().await?;
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/storage.rs` (new)

```rust
use crate::{AccountId, Near, NearToken, Error};
use serde::{Deserialize, Serialize};

/// Storage balance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBalance {
    pub total: NearToken,
    pub available: NearToken,
}

/// Storage balance bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBalanceBounds {
    pub min: NearToken,
    pub max: Option<NearToken>,
}

/// Storage deposit client for a specific contract
pub struct StorageDeposit<'a> {
    near: &'a Near,
    contract_id: AccountId,
}

impl<'a> StorageDeposit<'a> {
    pub fn new(near: &'a Near, contract_id: AccountId) -> Self {
        Self { near, contract_id }
    }
    
    /// Get storage balance bounds (storage_balance_bounds)
    pub async fn balance_bounds(&self) -> Result<StorageBalanceBounds, Error> {
        self.near
            .view(&self.contract_id, "storage_balance_bounds")
            .await
    }
    
    /// Get storage balance for an account (storage_balance_of)
    pub async fn balance_of(
        &self,
        account_id: impl AsRef<str>,
    ) -> Result<Option<StorageBalance>, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        self.near
            .view(&self.contract_id, "storage_balance_of")
            .args(Args { account_id: account_id.as_ref() })
            .await
    }
    
    /// Deposit storage for an account (storage_deposit)
    pub fn deposit(&self, account_id: impl AsRef<str>) -> StorageDepositCall<'a> {
        StorageDepositCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            account_id: Some(account_id.as_ref().to_string()),
            deposit: NearToken::millinear(100), // Reasonable default
            registration_only: false,
        }
    }
    
    /// Register an account with minimum storage (storage_deposit with registration_only)
    pub fn register(&self, account_id: impl AsRef<str>) -> StorageDepositCall<'a> {
        StorageDepositCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            account_id: Some(account_id.as_ref().to_string()),
            deposit: NearToken::millinear(100),
            registration_only: true,
        }
    }
    
    /// Withdraw storage deposit (storage_withdraw)
    pub fn withdraw(&self, amount: Option<NearToken>) -> StorageWithdrawCall<'a> {
        StorageWithdrawCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            amount,
        }
    }
    
    /// Unregister from contract (storage_unregister)
    pub fn unregister(&self) -> StorageUnregisterCall<'a> {
        StorageUnregisterCall {
            near: self.near,
            contract_id: self.contract_id.clone(),
            force: false,
        }
    }
}

/// Builder for storage deposit
pub struct StorageDepositCall<'a> {
    near: &'a Near,
    contract_id: AccountId,
    account_id: Option<String>,
    deposit: NearToken,
    registration_only: bool,
}

impl<'a> StorageDepositCall<'a> {
    pub fn deposit(mut self, amount: impl IntoNearToken) -> Self {
        self.deposit = amount.into_near_token().unwrap_or(NearToken::millinear(100));
        self
    }
    
    pub fn registration_only(mut self) -> Self {
        self.registration_only = true;
        self
    }
}

impl<'a> IntoFuture for StorageDepositCall<'a> {
    type Output = Result<StorageBalance, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;
    
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            #[derive(Serialize)]
            struct Args {
                #[serde(skip_serializing_if = "Option::is_none")]
                account_id: Option<String>,
                #[serde(skip_serializing_if = "std::ops::Not::not")]
                registration_only: bool,
            }
            
            // Call and parse result
            let outcome = self.near
                .call(&self.contract_id, "storage_deposit")
                .args(Args {
                    account_id: self.account_id,
                    registration_only: self.registration_only,
                })
                .deposit(self.deposit)
                .await?;
            
            // Parse return value as StorageBalance
            // ...
        })
    }
}

// Similar implementations for StorageWithdrawCall and StorageUnregisterCall

// Add to Near client
impl Near {
    /// Get a storage deposit client for a contract
    pub fn storage(&self, contract_id: impl TryInto<AccountId>) -> Result<StorageDeposit<'_>, Error> {
        Ok(StorageDeposit::new(self, contract_id.try_into()?))
    }
}
```

---

## 5. Staking Pool Operations

### Priority: 🟡 Important

### Description
API for interacting with NEAR staking pools - depositing, staking, unstaking, and withdrawing.

### Reference Implementation

**near-api-rs** - `api/src/stake.rs`:
```rust
// Pool info
Staking::staking_pool_info(pool_id).fetch().await?;
Staking::active_staking_pools().fetch().await?;

// Delegation
let delegation = Staking::delegation(account_id);
delegation.view_staked_balance(pool).fetch().await?;
delegation.deposit_and_stake(pool, amount).send().await?;
delegation.unstake(pool, amount).send().await?;
delegation.withdraw_all(pool).send().await?;
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/staking.rs` (new)

```rust
use crate::{AccountId, Near, NearToken, Gas, Error};
use serde::{Deserialize, Serialize};

/// Staking pool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingPoolInfo {
    pub validator_id: AccountId,
    pub fee: Option<RewardFeeFraction>,
    pub delegators: Option<u64>,
    pub stake: NearToken,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardFeeFraction {
    pub numerator: u32,
    pub denominator: u32,
}

/// User's stake balance in a pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakeBalance {
    pub staked: NearToken,
    pub unstaked: NearToken,
    pub total: NearToken,
    pub can_withdraw: bool,
}

/// Staking pool client
pub struct StakingPool<'a> {
    near: &'a Near,
    pool_id: AccountId,
}

impl<'a> StakingPool<'a> {
    pub fn new(near: &'a Near, pool_id: AccountId) -> Self {
        Self { near, pool_id }
    }
    
    /// Get pool's total staked balance
    pub async fn total_staked(&self) -> Result<NearToken, Error> {
        let balance: String = self.near
            .view(&self.pool_id, "get_total_staked_balance")
            .await?;
        Ok(NearToken::from_yoctonear(balance.parse()?))
    }
    
    /// Get reward fee fraction
    pub async fn reward_fee(&self) -> Result<RewardFeeFraction, Error> {
        self.near
            .view(&self.pool_id, "get_reward_fee_fraction")
            .await
    }
    
    /// Get account's staked balance
    pub async fn staked_balance(&self, account_id: impl AsRef<str>) -> Result<NearToken, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        let balance: String = self.near
            .view(&self.pool_id, "get_account_staked_balance")
            .args(Args { account_id: account_id.as_ref() })
            .await?;
        Ok(NearToken::from_yoctonear(balance.parse()?))
    }
    
    /// Get account's unstaked balance
    pub async fn unstaked_balance(&self, account_id: impl AsRef<str>) -> Result<NearToken, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        let balance: String = self.near
            .view(&self.pool_id, "get_account_unstaked_balance")
            .args(Args { account_id: account_id.as_ref() })
            .await?;
        Ok(NearToken::from_yoctonear(balance.parse()?))
    }
    
    /// Check if unstaked balance is available for withdrawal
    pub async fn is_unstaked_available(&self, account_id: impl AsRef<str>) -> Result<bool, Error> {
        #[derive(Serialize)]
        struct Args<'a> {
            account_id: &'a str,
        }
        
        self.near
            .view(&self.pool_id, "is_account_unstaked_balance_available")
            .args(Args { account_id: account_id.as_ref() })
            .await
    }
    
    /// Get full stake balance info
    pub async fn balance(&self, account_id: impl AsRef<str>) -> Result<StakeBalance, Error> {
        let account_id = account_id.as_ref();
        let (staked, unstaked, can_withdraw) = tokio::try_join!(
            self.staked_balance(account_id),
            self.unstaked_balance(account_id),
            self.is_unstaked_available(account_id),
        )?;
        
        Ok(StakeBalance {
            staked,
            unstaked,
            total: staked.saturating_add(unstaked),
            can_withdraw,
        })
    }
    
    /// Deposit NEAR to the pool (deposit)
    pub fn deposit(&self, amount: impl IntoNearToken) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "deposit",
            amount: None,
            deposit: amount.into_near_token().ok(),
        }
    }
    
    /// Deposit and stake in one call (deposit_and_stake)
    pub fn deposit_and_stake(&self, amount: impl IntoNearToken) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "deposit_and_stake",
            amount: None,
            deposit: amount.into_near_token().ok(),
        }
    }
    
    /// Stake unstaked balance (stake)
    pub fn stake(&self, amount: impl IntoNearToken) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "stake",
            amount: amount.into_near_token().ok(),
            deposit: None,
        }
    }
    
    /// Stake all unstaked balance (stake_all)
    pub fn stake_all(&self) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "stake_all",
            amount: None,
            deposit: None,
        }
    }
    
    /// Unstake staked balance (unstake)
    pub fn unstake(&self, amount: impl IntoNearToken) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "unstake",
            amount: amount.into_near_token().ok(),
            deposit: None,
        }
    }
    
    /// Unstake all staked balance (unstake_all)
    pub fn unstake_all(&self) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "unstake_all",
            amount: None,
            deposit: None,
        }
    }
    
    /// Withdraw unstaked balance (withdraw)
    pub fn withdraw(&self, amount: impl IntoNearToken) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "withdraw",
            amount: amount.into_near_token().ok(),
            deposit: None,
        }
    }
    
    /// Withdraw all unstaked balance (withdraw_all)
    pub fn withdraw_all(&self) -> StakingCall<'a> {
        StakingCall {
            near: self.near,
            pool_id: self.pool_id.clone(),
            method: "withdraw_all",
            amount: None,
            deposit: None,
        }
    }
}

/// Builder for staking transactions
pub struct StakingCall<'a> {
    near: &'a Near,
    pool_id: AccountId,
    method: &'static str,
    amount: Option<NearToken>,
    deposit: Option<NearToken>,
}

impl<'a> IntoFuture for StakingCall<'a> {
    type Output = Result<FinalExecutionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 'a>>;
    
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let mut call = self.near.call(&self.pool_id, self.method);
            
            if let Some(amount) = self.amount {
                #[derive(Serialize)]
                struct Args { amount: String }
                call = call.args(Args { amount: amount.as_yoctonear().to_string() });
            }
            
            if let Some(deposit) = self.deposit {
                call = call.deposit(deposit);
            }
            
            call.gas(Gas::tgas(50)).await
        })
    }
}

// Add to Near client
impl Near {
    /// Get a staking pool client
    pub fn staking(&self, pool_id: impl TryInto<AccountId>) -> Result<StakingPool<'_>, Error> {
        Ok(StakingPool::new(self, pool_id.try_into()?))
    }
}
```

---

## 6. Keystore / Keychain Signer ✅ IMPLEMENTED

### Status: Implemented

System keychain integration for secure key storage using OS-native credential managers.

### Implementation

**File**: `crates/near-kit/src/client/keyring_signer.rs`

```rust
use near_kit::{KeyringSigner, Near};

// Load a key stored by near-cli-rs
let signer = KeyringSigner::new(
    "testnet",
    "alice.testnet",
    "ed25519:6fWy..."
)?;

let near = Near::testnet().signer(signer).build();
near.transfer("bob.testnet", "1 NEAR").await?;
```

**Features:**
- 100% compatible with keys stored by `near-cli-rs`
- Feature-gated (`keyring` feature, enabled by default)
- Supports macOS Keychain, Windows Credential Manager, Linux Secret Service
- Parses both full (with seed phrase) and simple JSON formats

**Note:** Write operations (store/delete) are intentionally omitted. Use `near-cli-rs` for key management.

---

## 7. Ledger Hardware Wallet Support

### Priority: 🟢 Nice-to-Have

### Description
Support for Ledger hardware wallets for secure transaction signing.

### Reference Implementation

**near-api-rs** - `api/src/signer/ledger.rs`:
```rust
Signer::from_ledger()?;
Signer::from_ledger_with_hd_path(path)?;
// Supports: sign, sign_meta (NEP-366), sign_message_nep413
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/client/ledger_signer.rs` (new, feature-gated)

```rust
//! Ledger hardware wallet signer
//! 
//! Enabled with feature `ledger`

use near_ledger::NEARLedgerError;
use crate::{AccountId, PublicKey, Signature, Error};
use crate::client::signer::{Signer, SignFuture, Nep413SignFuture};

/// Default NEAR HD path for Ledger
pub const DEFAULT_LEDGER_HD_PATH: &str = "44'/397'/0'/0'/1'";

/// Signer using a Ledger hardware wallet
pub struct LedgerSigner {
    account_id: AccountId,
    public_key: PublicKey,
    hd_path: String,
}

impl LedgerSigner {
    /// Connect to Ledger with default HD path
    pub async fn new(account_id: impl TryInto<AccountId>) -> Result<Self, Error> {
        Self::with_hd_path(account_id, DEFAULT_LEDGER_HD_PATH).await
    }
    
    /// Connect to Ledger with custom HD path
    pub async fn with_hd_path(
        account_id: impl TryInto<AccountId>,
        hd_path: impl Into<String>,
    ) -> Result<Self, Error> {
        let account_id = account_id.try_into()?;
        let hd_path = hd_path.into();
        
        // Get public key from Ledger
        let public_key = near_ledger::get_public_key_with_display_flag(
            hd_path.parse().map_err(|e| Error::Ledger(format!("Invalid HD path: {}", e)))?,
            false, // Don't require confirmation for public key
        )
        .await
        .map_err(|e| Error::Ledger(e.to_string()))?;
        
        Ok(Self {
            account_id,
            public_key: PublicKey::ed25519_from_bytes(public_key.try_into().unwrap()),
            hd_path,
        })
    }
    
    /// Get the HD path being used
    pub fn hd_path(&self) -> &str {
        &self.hd_path
    }
}

impl Signer for LedgerSigner {
    fn account_id(&self) -> &AccountId {
        &self.account_id
    }
    
    fn public_key(&self) -> &PublicKey {
        &self.public_key
    }
    
    fn sign(&self, message: &[u8]) -> SignFuture<'_> {
        Box::pin(async move {
            let signature_bytes = near_ledger::sign_transaction(
                message.to_vec(),
                self.hd_path.parse().unwrap(),
            )
            .await
            .map_err(|e| crate::client::signer::SignerError::SigningFailed(e.to_string()))?;
            
            let signature = Signature::ed25519_from_bytes(
                signature_bytes.try_into().map_err(|_| {
                    crate::client::signer::SignerError::SigningFailed("Invalid signature length".into())
                })?
            );
            
            Ok((signature, self.public_key.clone()))
        })
    }
    
    fn sign_nep413<'a>(&'a self, params: &'a SignMessageParams) -> Nep413SignFuture<'a> {
        Box::pin(async move {
            // Use Ledger's NEP-413 signing support
            let nep413_payload = near_ledger::NEP413Payload {
                message: params.message.clone(),
                nonce: params.nonce,
                recipient: params.recipient.clone(),
                callback_url: params.callback_url.clone(),
            };
            
            let signature_bytes = near_ledger::sign_nep413_message(
                nep413_payload,
                self.hd_path.parse().unwrap(),
            )
            .await
            .map_err(|e| crate::client::signer::SignerError::SigningFailed(e.to_string()))?;
            
            Ok(SignedMessage {
                account_id: self.account_id.clone(),
                public_key: self.public_key.clone(),
                signature: Signature::ed25519_from_bytes(signature_bytes.try_into().unwrap()),
                state: params.state.clone(),
            })
        })
    }
}
```

**Cargo.toml addition**:
```toml
[features]
ledger = ["near-ledger"]

[dependencies]
near-ledger = { version = "0.5", optional = true }
```

---

## 8. Gas Keys (NEP-611)

### Priority: 🟢 Nice-to-Have

### Description
NEP-611 introduces gas keys - a new type of access key that can pay for gas on behalf of other accounts.

### Reference Implementation

**near-api-rs** - `types/src/transaction/actions.rs`:
```rust
pub struct AddGasKeyAction {
    pub public_key: PublicKey,
    pub nonce: Nonce,
    pub allowance: NearToken,
    pub receiver_id: AccountId,
}

pub struct DeleteGasKeyAction {
    pub public_key: PublicKey,
}

pub struct TransferToGasKeyAction {
    pub public_key: PublicKey,
    pub deposit: NearToken,
}
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/types/action.rs` (extend existing)

```rust
/// NEP-611: Add a gas key
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct AddGasKeyAction {
    pub public_key: PublicKey,
    pub nonce: u64,
    pub allowance: NearToken,
    pub receiver_id: AccountId,
}

/// NEP-611: Delete a gas key
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct DeleteGasKeyAction {
    pub public_key: PublicKey,
}

/// NEP-611: Transfer NEAR to a gas key
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize, Serialize, Deserialize)]
pub struct TransferToGasKeyAction {
    pub public_key: PublicKey,
    pub deposit: NearToken,
}

// Add to Action enum
pub enum Action {
    // ... existing variants ...
    AddGasKey(AddGasKeyAction),           // discriminant 12
    DeleteGasKey(DeleteGasKeyAction),     // discriminant 13
    TransferToGasKey(TransferToGasKeyAction), // discriminant 14
}

impl Action {
    /// Create an add gas key action (NEP-611)
    pub fn add_gas_key(
        public_key: PublicKey,
        allowance: NearToken,
        receiver_id: AccountId,
    ) -> Self {
        Self::AddGasKey(AddGasKeyAction {
            public_key,
            nonce: 0,
            allowance,
            receiver_id,
        })
    }
    
    /// Create a delete gas key action (NEP-611)
    pub fn delete_gas_key(public_key: PublicKey) -> Self {
        Self::DeleteGasKey(DeleteGasKeyAction { public_key })
    }
    
    /// Create a transfer to gas key action (NEP-611)
    pub fn transfer_to_gas_key(public_key: PublicKey, deposit: NearToken) -> Self {
        Self::TransferToGasKey(TransferToGasKeyAction { public_key, deposit })
    }
}
```

**File**: `crates/near-kit/src/client/transaction.rs` (extend TransactionBuilder)

```rust
impl TransactionBuilder {
    /// Add a gas key (NEP-611)
    pub fn add_gas_key(
        self,
        public_key: PublicKey,
        allowance: impl IntoNearToken,
        receiver_id: impl TryInto<AccountId>,
    ) -> Self {
        let action = Action::add_gas_key(
            public_key,
            allowance.into_near_token().unwrap_or(NearToken::ZERO),
            receiver_id.try_into().unwrap(),
        );
        self.add_action(action)
    }
    
    /// Delete a gas key (NEP-611)
    pub fn delete_gas_key(self, public_key: PublicKey) -> Self {
        self.add_action(Action::delete_gas_key(public_key))
    }
    
    /// Transfer NEAR to a gas key (NEP-611)
    pub fn transfer_to_gas_key(
        self,
        public_key: PublicKey,
        amount: impl IntoNearToken,
    ) -> Self {
        let action = Action::transfer_to_gas_key(
            public_key,
            amount.into_near_token().unwrap_or(NearToken::ZERO),
        );
        self.add_action(action)
    }
}
```

---

## 9. Transaction V1 with Priority Fee

### Priority: 🟢 Nice-to-Have

### Description
NEAR Protocol now supports Transaction V1 which includes a priority fee field for transaction ordering.

### Reference Implementation

**near-api-rs** - `types/src/transaction/mod.rs`:
```rust
pub enum Transaction {
    V0(TransactionV0),
    V1(TransactionV1),
}

pub struct TransactionV1 {
    pub signer_id: AccountId,
    pub public_key: PublicKey,
    pub nonce: Nonce,
    pub receiver_id: AccountId,
    pub block_hash: CryptoHash,
    pub actions: Vec<Action>,
    pub priority_fee: u64,  // NEW
}
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/types/transaction.rs` (extend existing)

```rust
/// Transaction version enum
#[derive(Debug, Clone)]
pub enum TransactionVersion {
    V0(Transaction),
    V1(TransactionV1),
}

/// Transaction V1 with priority fee support
#[derive(Debug, Clone, BorshSerialize, BorshDeserialize)]
pub struct TransactionV1 {
    pub signer_id: AccountId,
    pub public_key: PublicKey,
    pub nonce: u64,
    pub receiver_id: AccountId,
    pub block_hash: CryptoHash,
    pub actions: Vec<Action>,
    pub priority_fee: u64,
}

impl TransactionV1 {
    pub fn new(
        signer_id: AccountId,
        public_key: PublicKey,
        nonce: u64,
        receiver_id: AccountId,
        block_hash: CryptoHash,
        actions: Vec<Action>,
        priority_fee: u64,
    ) -> Self {
        Self {
            signer_id,
            public_key,
            nonce,
            receiver_id,
            block_hash,
            actions,
            priority_fee,
        }
    }
    
    pub fn get_hash(&self) -> CryptoHash {
        CryptoHash::hash(&borsh::to_vec(self).unwrap())
    }
    
    pub fn sign(self, signer: &SecretKey) -> SignedTransactionV1 {
        let hash = self.get_hash();
        let signature = signer.sign(hash.as_bytes());
        SignedTransactionV1 {
            transaction: self,
            signature,
        }
    }
}
```

**File**: `crates/near-kit/src/client/transaction.rs` (extend TransactionBuilder)

```rust
impl TransactionBuilder {
    /// Set priority fee for the transaction (creates V1 transaction)
    pub fn priority_fee(mut self, fee: u64) -> Self {
        self.priority_fee = Some(fee);
        self
    }
}
```

---

## 10. Contract ABI Support

### Priority: 🟢 Nice-to-Have

### Description
Ability to fetch and parse contract ABIs for type-safe contract interactions.

### Reference Implementation

**near-api-rs** - `api/src/contract.rs`:
```rust
Contract(contract_id).abi().fetch_from_testnet().await?;
// Returns decompressed ABI from __contract_abi method
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/abi.rs` (new)

```rust
use crate::{AccountId, Near, Error};
use serde::{Deserialize, Serialize};

/// Contract ABI root structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAbi {
    pub schema_version: String,
    pub metadata: AbiMetadata,
    pub body: AbiBody,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiMetadata {
    pub name: Option<String>,
    pub version: Option<String>,
    pub authors: Vec<String>,
    pub build: Option<AbiBuildInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiBuildInfo {
    pub compiler: String,
    pub builder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiBody {
    pub functions: Vec<AbiFunction>,
    pub root_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiFunction {
    pub name: String,
    pub kind: AbiFunctionKind,
    pub params: Vec<AbiParameter>,
    pub result: Option<AbiType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AbiFunctionKind {
    View,
    Call,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiParameter {
    pub name: String,
    #[serde(rename = "type_schema")]
    pub type_schema: AbiType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbiType {
    #[serde(rename = "$ref")]
    pub reference: Option<String>,
    #[serde(flatten)]
    pub schema: serde_json::Value,
}

impl Near {
    /// Fetch the ABI for a contract
    pub async fn contract_abi(&self, contract_id: impl TryInto<AccountId>) -> Result<ContractAbi, Error> {
        let contract_id = contract_id.try_into()?;
        
        // Call __contract_abi view function
        let compressed: Vec<u8> = self
            .view::<Vec<u8>>(&contract_id, "__contract_abi")
            .await
            .map_err(|_| Error::NoContractAbi)?;
        
        // Decompress with zstd
        let decompressed = zstd::decode_all(compressed.as_slice())
            .map_err(|e| Error::AbiDecompression(e.to_string()))?;
        
        // Parse JSON
        serde_json::from_slice(&decompressed)
            .map_err(|e| Error::AbiParse(e.to_string()))
    }
}
```

**Cargo.toml addition**:
```toml
[dependencies]
zstd = "0.13"
```

---

## 11. View State / Storage Queries

### Priority: 🟢 Nice-to-Have

### Description
Direct access to contract storage state.

### Reference Implementation

**near-api-rs** - `api/src/contract.rs`:
```rust
Contract(id).view_storage().fetch().await?;
Contract(id).view_storage_with_prefix(prefix).fetch().await?;
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/client/rpc.rs` (extend RpcClient)

```rust
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};

/// State item from contract storage
#[derive(Debug, Clone, Deserialize)]
pub struct StateItem {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViewStateResult {
    pub values: Vec<StateItem>,
}

impl RpcClient {
    /// View contract state
    pub async fn view_state(
        &self,
        account_id: &AccountId,
        prefix: Option<&[u8]>,
        block: BlockReference,
    ) -> Result<ViewStateResult, RpcError> {
        #[derive(Serialize)]
        struct Params<'a> {
            request_type: &'static str,
            account_id: &'a str,
            prefix_base64: String,
            #[serde(flatten)]
            block: BlockReferenceParam,
        }
        
        self.call("query", Params {
            request_type: "view_state",
            account_id: account_id.as_str(),
            prefix_base64: BASE64.encode(prefix.unwrap_or(&[])),
            block: block.into(),
        }).await
    }
}
```

---

## 12. Presigning / Offline Signing ✅ IMPLEMENTED

### Status: Implemented

Sign transactions without network access, useful for air-gapped signing and multi-signature workflows.

### Implementation

**File**: `crates/near-kit/src/client/transaction.rs`

```rust
use near_kit::*;

// Step 1: On online machine - get block_hash and nonce
let block = near.rpc().block(BlockReference::Finality(Finality::Final)).await?;
let block_hash = block.header.hash;

let access_key = near.rpc().view_access_key(&account_id, &public_key, ...).await?;
let nonce = access_key.nonce + 1;

// Step 2: On offline machine - sign without network
let signed = near.transaction("receiver.near")
    .transfer(NearToken::near(1))
    .sign_offline(block_hash, nonce)?;

// Step 3: Serialize for transport
let payload = signed.to_base64();

// Step 4: On online machine - deserialize and send
let received_tx = SignedTransaction::from_base64(&payload)?;
near.send(&received_tx).await?;
```

**Features:**
- `TransactionBuilder::sign_offline(block_hash, nonce)` - Sign without network access
- `CallBuilder::sign_offline(block_hash, nonce)` - Sign function calls offline
- `SignedTransaction::from_bytes()` / `from_base64()` - Deserialize signed transactions
- Full round-trip serialization support

---

## 13. Environment-Based Configuration

### Priority: 🟢 Nice-to-Have

### Description
Configure the Near client from environment variables.

### Reference Implementation

**near-kit (TypeScript)**:
```typescript
// Respects NEAR_NETWORK env var for network selection
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/client/near.rs` (extend Near)

```rust
impl Near {
    /// Create a Near client from environment variables
    /// 
    /// Environment variables:
    /// - `NEAR_NETWORK`: "mainnet", "testnet", or RPC URL (default: "testnet")
    /// - `NEAR_ACCOUNT_ID`: Signer account ID
    /// - `NEAR_PRIVATE_KEY`: Signer private key (ed25519:...)
    /// - `NEAR_SEED_PHRASE`: Alternative to private key
    pub fn from_env() -> Result<Near, Error> {
        let network = std::env::var("NEAR_NETWORK").unwrap_or_else(|_| "testnet".to_string());
        
        let builder = match network.as_str() {
            "mainnet" => Near::mainnet(),
            "testnet" => Near::testnet(),
            url if url.starts_with("http") => Near::custom(url),
            _ => return Err(Error::Config(format!("Unknown network: {}", network))),
        };
        
        // Try to set up signer from env
        let builder = if let Ok(account_id) = std::env::var("NEAR_ACCOUNT_ID") {
            if let Ok(private_key) = std::env::var("NEAR_PRIVATE_KEY") {
                builder.credentials(&private_key, &account_id)?
            } else if let Ok(seed_phrase) = std::env::var("NEAR_SEED_PHRASE") {
                let secret = SecretKey::from_seed_phrase(&seed_phrase)?;
                builder.signer(InMemorySigner::from_secret_key(account_id.parse()?, secret))
            } else {
                builder
            }
        } else {
            builder
        };
        
        Ok(builder.build())
    }
}
```

---

## 14. Contract Source Metadata (NEP-330)

### Priority: 🟢 Nice-to-Have

### Description
Query contract source metadata for verification and transparency.

### Reference Implementation

**near-api-rs** - `types/src/contract.rs`:
```rust
pub struct ContractSourceMetadata {
    pub version: Option<String>,
    pub link: Option<String>,
    pub standards: Vec<Standard>,
    pub build_info: Option<BuildInfo>,
}
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/types/contract.rs` (new)

```rust
use serde::{Deserialize, Serialize};

/// NEP-330 Contract source metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractSourceMetadata {
    pub version: Option<String>,
    pub link: Option<String>,
    pub standards: Vec<Standard>,
    pub build_info: Option<BuildInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Standard {
    pub standard: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub build_environment: String,
    pub build_command: Vec<String>,
    pub contract_path: String,
    pub source_code_snapshot: String,
    pub output_wasm_path: Option<String>,
}

impl Near {
    /// Get contract source metadata (NEP-330)
    pub async fn contract_source_metadata(
        &self,
        contract_id: impl TryInto<AccountId>,
    ) -> Result<ContractSourceMetadata, Error> {
        self.view(contract_id.try_into()?, "contract_source_metadata").await
    }
}
```

---

## 15. FT Amount Type with Decimal Handling

### Priority: 🟢 Nice-to-Have

### Description
A dedicated type for fungible token amounts with proper decimal handling.

### Reference Implementation

**near-api-rs** - `types/src/tokens.rs`:
```rust
pub struct FTBalance {
    amount: u128,
    decimals: u8,
    symbol: &'static str,
}

// Pre-defined constants
pub const USDT_BALANCE: FTBalance = FTBalance::new(6, "USDT");
pub const USDC_BALANCE: FTBalance = FTBalance::new(6, "USDC");
pub const W_NEAR_BALANCE: FTBalance = FTBalance::new(24, "wNEAR");
```

### Proposed API for near-kit-rs

**File**: `crates/near-kit/src/types/ft_amount.rs` (new)

```rust
use std::fmt;

/// Amount of a fungible token with decimal precision
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FtAmount {
    /// Raw amount in smallest units
    pub raw: u128,
    /// Number of decimal places
    pub decimals: u8,
}

impl FtAmount {
    pub const fn new(decimals: u8) -> Self {
        Self { raw: 0, decimals }
    }
    
    /// Create from raw amount (smallest units)
    pub const fn from_raw(raw: u128, decimals: u8) -> Self {
        Self { raw, decimals }
    }
    
    /// Create from whole units
    pub fn from_whole(whole: u128, decimals: u8) -> Self {
        Self {
            raw: whole * 10u128.pow(decimals as u32),
            decimals,
        }
    }
    
    /// Create from a float string (e.g., "1.5")
    pub fn from_float_str(s: &str, decimals: u8) -> Result<Self, ParseAmountError> {
        let parts: Vec<&str> = s.split('.').collect();
        
        let whole: u128 = parts[0].parse().map_err(|_| ParseAmountError::InvalidNumber)?;
        
        let fractional = if parts.len() > 1 {
            let frac_str = parts[1];
            if frac_str.len() > decimals as usize {
                return Err(ParseAmountError::TooManyDecimals);
            }
            
            let padded = format!("{:0<width$}", frac_str, width = decimals as usize);
            padded.parse::<u128>().map_err(|_| ParseAmountError::InvalidNumber)?
        } else {
            0
        };
        
        let raw = whole * 10u128.pow(decimals as u32) + fractional;
        Ok(Self { raw, decimals })
    }
    
    /// Get the raw amount
    pub const fn as_raw(&self) -> u128 {
        self.raw
    }
    
    /// Get the whole unit amount (truncated)
    pub fn as_whole(&self) -> u128 {
        self.raw / 10u128.pow(self.decimals as u32)
    }
    
    /// Get as float (may lose precision for large amounts)
    pub fn as_float(&self) -> f64 {
        self.raw as f64 / 10f64.powi(self.decimals as i32)
    }
}

impl fmt::Display for FtAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let divisor = 10u128.pow(self.decimals as u32);
        let whole = self.raw / divisor;
        let frac = self.raw % divisor;
        
        if frac == 0 {
            write!(f, "{}", whole)
        } else {
            write!(f, "{}.{:0>width$}", whole, frac, width = self.decimals as usize)
        }
    }
}

// Common token amount constructors
pub mod tokens {
    use super::FtAmount;
    
    /// USDT (6 decimals)
    pub fn usdt(amount: &str) -> Result<FtAmount, super::ParseAmountError> {
        FtAmount::from_float_str(amount, 6)
    }
    
    /// USDC (6 decimals)
    pub fn usdc(amount: &str) -> Result<FtAmount, super::ParseAmountError> {
        FtAmount::from_float_str(amount, 6)
    }
    
    /// wNEAR (24 decimals)
    pub fn wnear(amount: &str) -> Result<FtAmount, super::ParseAmountError> {
        FtAmount::from_float_str(amount, 24)
    }
}
```

---

## Implementation Priority Summary

| Priority | Feature | Effort | Dependencies |
|----------|---------|--------|--------------|
| 🔴 Critical | Seed Phrase/BIP39 | Medium | bip39, slip10_ed25519 |
| 🔴 Critical | FT Helpers (NEP-141) | Medium | None |
| 🔴 Critical | NFT Helpers (NEP-171) | Medium | None |
| 🟡 Important | Storage Deposits (NEP-145) | Low | None |
| 🟡 Important | Staking Operations | Medium | None |
| 🟡 Important | Keystore Signer | Low | keyring (optional) |
| 🟢 Nice-to-Have | Ledger Support | Medium | near-ledger (optional) |
| 🟢 Nice-to-Have | Gas Keys (NEP-611) | Low | None |
| 🟢 Nice-to-Have | Transaction V1 | Low | None |
| 🟢 Nice-to-Have | Contract ABI | Low | zstd |
| 🟢 Nice-to-Have | View State | Low | None |
| 🟢 Nice-to-Have | Presigning | Low | None |
| 🟢 Nice-to-Have | Env Configuration | Low | None |
| 🟢 Nice-to-Have | NEP-330 Metadata | Low | None |
| 🟢 Nice-to-Have | FT Amount Type | Low | None |

---

## Testing Requirements

Each feature should include:

1. **Unit tests** for type parsing and validation
2. **Integration tests** using the sandbox
3. **Documentation examples** in doc comments

Example test structure:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_unit_behavior() { ... }
    
    #[tokio::test]
    #[cfg(feature = "sandbox")]
    async fn test_integration_with_sandbox() {
        let sandbox = SandboxConfig::shared().await;
        // Deploy contract, test feature
    }
}
```

---

## Notes for Implementers

1. **Follow existing patterns** - Look at how similar features are implemented in the codebase
2. **Use IntoFuture** - All async operations should implement `IntoFuture` for ergonomic `.await`
3. **Accept flexible types** - Use `impl TryInto<AccountId>`, `impl IntoNearToken`, etc.
4. **Error handling** - Add new error variants to `Error` enum as needed
5. **Feature gates** - Use Cargo features for optional dependencies (ledger, keystore)
6. **Documentation** - Add doc comments with examples for all public APIs
7. **Commit regularly** - Commit after completing each feature with semantic commit messages
