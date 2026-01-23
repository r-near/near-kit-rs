# near-kit-rs: Complete Specification

> A ground-up Rust implementation of a NEAR Protocol client library, inspired by [near-kit](https://github.com/peterargue/near-kit) (TypeScript).

## Project Overview

### What This Is

**near-kit-rs** is a completely new Rust crate for interacting with NEAR Protocol. It is NOT a refactor of `near-api-rs`—it is a clean-slate implementation built from first principles.

### Goals

1. **Idiomatic Rust**: Leverage the type system, traits, and patterns Rust developers expect
2. **Single entry point**: Everything flows through a `Near` client
3. **Type-safe but ergonomic**: Strong types without excessive verbosity
4. **RPC-first types**: Hand-rolled types based on actual NEAR RPC responses, not inherited from other crates
5. **Minimal dependencies**: Only what's truly needed
6. **Excellent DX**: Great error messages, IDE discoverability, clear documentation

### Non-Goals

- Backwards compatibility with `near-api-rs`
- Reusing types from `near-primitives` or other NEAR crates
- Supporting every possible use case in v1 (start focused, expand later)

---

## Reference Implementation: near-kit (TypeScript)

The TypeScript library at `/home/ricky/near-kit` serves as the primary design reference. Key files to study:

| File | Purpose |
|------|---------|
| `/home/ricky/near-kit/src/core/near.ts` | Main `Near` class - the single entry point |
| `/home/ricky/near-kit/src/core/rpc/rpc.ts` | RPC client with retry logic, error handling |
| `/home/ricky/near-kit/src/core/transaction.ts` | `TransactionBuilder` - fluent transaction API |
| `/home/ricky/near-kit/src/core/actions.ts` | Action factory functions |
| `/home/ricky/near-kit/src/core/schema.ts` | Borsh serialization schemas |
| `/home/ricky/near-kit/src/core/types.ts` | Core type definitions |
| `/home/ricky/near-kit/src/core/config-schemas.ts` | Configuration types, `BlockReference`, `Finality` |
| `/home/ricky/near-kit/src/utils/amount.ts` | NEAR amount parsing ("5 NEAR" → yoctoNEAR) |
| `/home/ricky/near-kit/src/utils/validation.ts` | Gas parsing ("30 Tgas" → gas units) |
| `/home/ricky/near-kit/src/errors/` | Error types and RPC error parsing |

### Key Patterns from near-kit

1. **Amount handling**: Accepts `"10 NEAR"`, `"1000 yocto"`, or `bigint` - explicit units required
2. **Gas handling**: Accepts `"30 Tgas"`, `"5 Ggas"` strings
3. **Args**: `object | Uint8Array` - JSON serialized by default, raw bytes for borsh
4. **BlockReference**: `{ finality: "final" | "optimistic" | "near-final" }` OR `{ blockId: number | string }`
5. **Transaction builder**: Fluent API that collects actions, then `.send()` executes

---

## Architecture

### Crate Structure

This is a single crate for simplicity, with a separate proc-macro crate for typed contracts.

```
near-kit-rs/
├── Cargo.toml              # Main crate
├── src/
│   ├── lib.rs              # Main exports and prelude
│   ├── error.rs            # Error types (Error, RpcError, Parse*Error)
│   ├── types/
│   │   ├── mod.rs          # Re-exports all types
│   │   ├── account.rs      # AccountId with validation
│   │   ├── units.rs        # NearToken, Gas with string parsing
│   │   ├── hash.rs         # CryptoHash (SHA-256)
│   │   ├── key.rs          # PublicKey, SecretKey, Signature
│   │   ├── action.rs       # Transaction actions (Transfer, FunctionCall, etc.)
│   │   ├── transaction.rs  # Transaction, SignedTransaction
│   │   ├── block_reference.rs  # BlockReference, Finality, TxExecutionStatus
│   │   └── rpc.rs          # RPC response types (AccountView, BlockView, etc.)
│   ├── client/
│   │   ├── mod.rs          # Re-exports client components
│   │   ├── near.rs         # Near client and NearBuilder
│   │   ├── rpc.rs          # RpcClient with retry logic
│   │   └── signer.rs       # Signer trait and SecretKeySigner
│   └── contract.rs         # Contract marker trait for typed contracts
├── near-kit-macros/        # Proc macro crate for #[near_kit::contract]
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs          # Proc macro implementation
├── examples/               # Example code
├── tests/                  # Integration tests
├── SPEC.md                 # This file
├── REFERENCE.md            # TypeScript patterns reference
└── AGENTS.md               # AI agent instructions
```

### Dependency Philosophy

**Allowed dependencies:**
- `tokio` - async runtime
- `reqwest` - HTTP client
- `serde`, `serde_json` - JSON serialization
- `borsh` - Borsh serialization (NEAR's binary format)
- `base64` - Base64 encoding for RPC
- `thiserror` - Error derive macro
- `bs58` - Base58 encoding
- `ed25519-dalek` - Ed25519 signatures
- `sha2` - SHA-256 hashing
- `hex` - Hex encoding
- `rand` - Random number generation (for key generation)

**NOT allowed:**
- `near-primitives` - We're hand-rolling types
- `near-crypto` - We're hand-rolling crypto types
- `near-jsonrpc-client` - We're building our own RPC client
- Any NEAR official crates (we start from scratch)

---

## Core Types (Hand-Rolled from RPC)

### Why Hand-Roll?

The existing `near-primitives` crate is designed for the NEAR node implementation. It's heavy, has many dependencies, and includes internal types not relevant for a client library. We want types that:

1. Match what the RPC actually returns (not internal node types)
2. Are minimal and focused on client use cases
3. Have excellent `Display` and `Debug` implementations
4. Support `serde` for JSON and `borsh` for binary encoding

### AccountId

```rust
/// A NEAR account identifier.
/// 
/// Valid account IDs:
/// - Named: "alice.near", "bob.testnet", "sub.account.near"
/// - Implicit (64 hex chars): "0123456789abcdef..."
/// - EVM (0x prefix): "0x1234..."
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AccountId(String);

impl AccountId {
    /// Parse and validate an account ID.
    pub fn new(s: impl Into<String>) -> Result<Self, ParseAccountIdError>;
    
    /// Create without validation (for internal use / testing).
    pub(crate) fn new_unchecked(s: impl Into<String>) -> Self;
    
    /// Check if this is an implicit account (64 hex chars).
    pub fn is_implicit(&self) -> bool;
    
    /// Check if this is a named account.
    pub fn is_named(&self) -> bool;
    
    /// Get the parent account (e.g., "sub.alice.near" → "alice.near").
    pub fn parent(&self) -> Option<AccountId>;
    
    /// Get as string slice.
    pub fn as_str(&self) -> &str;
}

impl FromStr for AccountId {
    type Err = ParseAccountIdError;
    fn from_str(s: &str) -> Result<Self, Self::Err> { ... }
}

impl TryFrom<&str> for AccountId { ... }
impl TryFrom<String> for AccountId { ... }

impl Display for AccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Borsh: serialize as string
impl BorshSerialize for AccountId { ... }
impl BorshDeserialize for AccountId { ... }
```

### NearToken (Amount)

```rust
/// A NEAR token amount with yoctoNEAR precision (10^-24 NEAR).
/// 
/// # Parsing
/// 
/// Supports parsing from strings with explicit units:
/// - `"5 NEAR"` or `"5 near"` - whole NEAR
/// - `"1.5 NEAR"` - decimal NEAR  
/// - `"500 milliNEAR"` or `"500 mNEAR"` - milliNEAR
/// - `"1000 yocto"` or `"1000 yoctoNEAR"` - yoctoNEAR
/// 
/// Raw numbers are NOT accepted to prevent unit confusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NearToken(u128);

impl NearToken {
    pub const ZERO: Self = Self(0);
    pub const ONE_YOCTO: Self = Self(1);
    pub const ONE_MILLINEAR: Self = Self(1_000_000_000_000_000_000_000); // 10^21
    pub const ONE_NEAR: Self = Self(1_000_000_000_000_000_000_000_000); // 10^24
    
    pub const fn from_yoctonear(yocto: u128) -> Self { Self(yocto) }
    pub const fn from_millinear(millinear: u128) -> Self { 
        Self(millinear * 1_000_000_000_000_000_000_000) 
    }
    pub const fn from_near(near: u128) -> Self { 
        Self(near * 1_000_000_000_000_000_000_000_000) 
    }
    
    /// Parse from decimal NEAR (e.g., 1.5 NEAR).
    pub fn from_near_decimal(near: f64) -> Result<Self, ParseAmountError>;
    
    pub const fn as_yoctonear(&self) -> u128 { self.0 }
    pub fn as_near_f64(&self) -> f64 { self.0 as f64 / 1e24 }
    
    pub fn checked_add(self, other: Self) -> Option<Self>;
    pub fn checked_sub(self, other: Self) -> Option<Self>;
    pub fn saturating_add(self, other: Self) -> Self;
    pub fn saturating_sub(self, other: Self) -> Self;
}

impl FromStr for NearToken {
    type Err = ParseAmountError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        
        // "X NEAR" or "X near"
        if let Some(value) = s.strip_suffix(" NEAR").or_else(|| s.strip_suffix(" near")) {
            return Self::from_near_decimal(value.parse()?);
        }
        
        // "X milliNEAR" or "X mNEAR"
        if let Some(value) = s.strip_suffix(" milliNEAR").or_else(|| s.strip_suffix(" mNEAR")) {
            return Ok(Self::from_millinear(value.trim().parse()?));
        }
        
        // "X yocto" or "X yoctoNEAR"
        if let Some(value) = s.strip_suffix(" yocto").or_else(|| s.strip_suffix(" yoctoNEAR")) {
            return Ok(Self::from_yoctonear(value.trim().parse()?));
        }
        
        // Bare number = error (ambiguous)
        if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(ParseAmountError::AmbiguousAmount(s.to_string()));
        }
        
        Err(ParseAmountError::InvalidFormat(s.to_string()))
    }
}

impl TryFrom<&str> for NearToken {
    type Error = ParseAmountError;
    fn try_from(s: &str) -> Result<Self, Self::Error> { s.parse() }
}

impl Display for NearToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            return write!(f, "0 NEAR");
        }
        
        let near = self.0 / Self::ONE_NEAR.0;
        let remainder = self.0 % Self::ONE_NEAR.0;
        
        if remainder == 0 {
            write!(f, "{} NEAR", near)
        } else {
            // Show up to 5 decimal places, trim trailing zeros
            let decimal = format!("{:024}", remainder);
            let decimal = decimal.trim_end_matches('0');
            let decimal = &decimal[..decimal.len().min(5)];
            write!(f, "{}.{} NEAR", near, decimal)
        }
    }
}

// Serde: serialize as string (yoctoNEAR)
impl Serialize for NearToken {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for NearToken {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ok(Self(s.parse().map_err(serde::de::Error::custom)?))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseAmountError {
    #[error("Ambiguous amount '{0}'. Use explicit units like '5 NEAR' or '1000 yocto'")]
    AmbiguousAmount(String),
    
    #[error("Invalid amount format: '{0}'")]
    InvalidFormat(String),
    
    #[error("Invalid number: {0}")]
    InvalidNumber(#[from] std::num::ParseFloatError),
}
```

### Gas

```rust
/// Gas units for NEAR transactions.
/// 
/// # Parsing
/// 
/// Supports parsing from strings:
/// - `"30 Tgas"` or `"30 tgas"` - teragas (10^12)
/// - `"5 Ggas"` or `"5 ggas"` - gigagas (10^9)
/// - `"1000000 gas"` - raw gas units
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Gas(u64);

impl Gas {
    pub const ZERO: Self = Self(0);
    pub const ONE_GGAS: Self = Self(1_000_000_000);
    pub const ONE_TGAS: Self = Self(1_000_000_000_000);
    
    /// Default gas for function calls (30 Tgas).
    pub const DEFAULT: Self = Self::from_tgas(30);
    
    /// Maximum gas per transaction (300 Tgas).
    pub const MAX: Self = Self::from_tgas(300);
    
    pub const fn from_gas(gas: u64) -> Self { Self(gas) }
    pub const fn from_ggas(ggas: u64) -> Self { Self(ggas * 1_000_000_000) }
    pub const fn from_tgas(tgas: u64) -> Self { Self(tgas * 1_000_000_000_000) }
    
    pub const fn as_gas(&self) -> u64 { self.0 }
    pub const fn as_tgas(&self) -> u64 { self.0 / 1_000_000_000_000 }
}

impl FromStr for Gas {
    type Err = ParseGasError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        
        if let Some(value) = s.strip_suffix(" Tgas").or_else(|| s.strip_suffix(" tgas")) {
            return Ok(Self::from_tgas(value.trim().parse()?));
        }
        
        if let Some(value) = s.strip_suffix(" Ggas").or_else(|| s.strip_suffix(" ggas")) {
            return Ok(Self::from_ggas(value.trim().parse()?));
        }
        
        if let Some(value) = s.strip_suffix(" gas") {
            return Ok(Self::from_gas(value.trim().parse()?));
        }
        
        Err(ParseGasError::InvalidFormat(s.to_string()))
    }
}

impl TryFrom<&str> for Gas {
    type Error = ParseGasError;
    fn try_from(s: &str) -> Result<Self, Self::Error> { s.parse() }
}

impl Display for Gas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tgas = self.0 / Self::ONE_TGAS.0;
        if tgas > 0 && self.0 % Self::ONE_TGAS.0 == 0 {
            write!(f, "{} Tgas", tgas)
        } else {
            write!(f, "{} gas", self.0)
        }
    }
}
```

### Keys and Signatures

```rust
/// Ed25519 public key.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct PublicKey {
    key_type: KeyType,
    data: Vec<u8>,
}

impl PublicKey {
    /// Parse from string like "ed25519:..." or "secp256k1:...".
    pub fn from_str(s: &str) -> Result<Self, ParseKeyError>;
    
    /// Create from raw bytes (Ed25519).
    pub fn ed25519_from_bytes(bytes: [u8; 32]) -> Self;
    
    /// Get the key type.
    pub fn key_type(&self) -> KeyType;
    
    /// Get the raw bytes.
    pub fn as_bytes(&self) -> &[u8];
}

impl Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.key_type {
            KeyType::Ed25519 => write!(f, "ed25519:{}", bs58::encode(&self.data).into_string()),
            KeyType::Secp256k1 => write!(f, "secp256k1:{}", bs58::encode(&self.data).into_string()),
        }
    }
}

/// Secret key for signing.
#[derive(Clone)]
pub struct SecretKey {
    key_type: KeyType,
    data: Vec<u8>,  // 32 bytes for ed25519, 32 for secp256k1
}

impl SecretKey {
    /// Parse from string like "ed25519:..." or "secp256k1:...".
    pub fn from_str(s: &str) -> Result<Self, ParseKeyError>;
    
    /// Generate a new random Ed25519 key pair.
    pub fn generate_ed25519() -> Self;
    
    /// Derive the public key.
    pub fn public_key(&self) -> PublicKey;
    
    /// Sign a message (returns 64-byte signature for ed25519).
    pub fn sign(&self, message: &[u8]) -> Signature;
}

/// Signature from signing.
#[derive(Clone)]
pub struct Signature {
    key_type: KeyType,
    data: Vec<u8>,  // 64 bytes for ed25519, 65 for secp256k1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyType {
    Ed25519 = 0,
    Secp256k1 = 1,
}
```

### CryptoHash

```rust
/// A 32-byte SHA-256 hash used for block hashes, transaction hashes, etc.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CryptoHash([u8; 32]);

impl CryptoHash {
    pub const ZERO: Self = Self([0; 32]);
    
    pub fn hash(data: &[u8]) -> Self {
        use sha2::{Sha256, Digest};
        Self(Sha256::digest(data).into())
    }
    
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}

impl FromStr for CryptoHash {
    type Err = ParseHashError;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = bs58::decode(s).into_vec()?;
        if bytes.len() != 32 {
            return Err(ParseHashError::InvalidLength(bytes.len()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }
}

impl Display for CryptoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", bs58::encode(&self.0).into_string())
    }
}

impl Debug for CryptoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CryptoHash({})", self)
    }
}
```

### Block Reference

```rust
/// Reference to a specific block for RPC queries.
/// 
/// Every NEAR RPC query operates on state at a specific block.
#[derive(Clone, Debug, Default)]
pub enum BlockReference {
    /// Query at latest block with specified finality.
    #[default]
    Finality(Finality),
    
    /// Query at specific block height.
    Height(u64),
    
    /// Query at specific block hash.
    Hash(CryptoHash),
}

/// Finality level for queries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Finality {
    /// Latest optimistic block. Fastest, but may be reorged.
    Optimistic,
    
    /// Doomslug finality. Irreversible unless validator slashed.
    NearFinal,
    
    /// Fully finalized. Slowest, 100% guaranteed.
    #[default]
    Final,
}

impl From<Finality> for BlockReference {
    fn from(f: Finality) -> Self { Self::Finality(f) }
}

impl From<u64> for BlockReference {
    fn from(height: u64) -> Self { Self::Height(height) }
}

impl From<CryptoHash> for BlockReference {
    fn from(hash: CryptoHash) -> Self { Self::Hash(hash) }
}

/// Transaction execution status for send_tx wait_until parameter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TxExecutionStatus {
    /// Don't wait, return immediately after RPC accepts.
    None,
    /// Wait for inclusion in a block.
    Included,
    /// Wait for execution (optimistic).
    #[default]
    ExecutedOptimistic,
    /// Wait for inclusion in final block.
    IncludedFinal,
    /// Wait for execution in final block.
    Executed,
    /// Wait for full finality.
    Final,
}

impl TxExecutionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Included => "INCLUDED",
            Self::ExecutedOptimistic => "EXECUTED_OPTIMISTIC",
            Self::IncludedFinal => "INCLUDED_FINAL",
            Self::Executed => "EXECUTED",
            Self::Final => "FINAL",
        }
    }
}
```

### RPC Response Types

These should be derived from actual RPC responses. Here's an example:

```rust
// Based on RPC response for `query { request_type: "view_account" }`
#[derive(Debug, Clone, Deserialize)]
pub struct AccountView {
    pub amount: NearToken,
    pub locked: NearToken,
    pub code_hash: CryptoHash,
    pub storage_usage: u64,
    pub storage_paid_at: u64,
    pub block_height: u64,
    pub block_hash: CryptoHash,
}

// Based on RPC response for `query { request_type: "view_access_key" }`
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyView {
    pub nonce: u64,
    pub permission: AccessKeyPermission,
    pub block_height: u64,
    pub block_hash: CryptoHash,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessKeyPermission {
    FullAccess,
    FunctionCall {
        allowance: Option<NearToken>,
        receiver_id: AccountId,
        method_names: Vec<String>,
    },
}

// Based on RPC response for send_tx
#[derive(Debug, Clone, Deserialize)]
pub struct FinalExecutionOutcome {
    pub status: ExecutionStatus,
    pub transaction: TransactionView,
    pub transaction_outcome: ExecutionOutcomeWithId,
    pub receipts_outcome: Vec<ExecutionOutcomeWithId>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ExecutionStatus {
    Unknown,
    Failure(serde_json::Value),
    SuccessValue(String),  // base64 encoded
    SuccessReceiptId(CryptoHash),
}
```

---

## RPC Client

### Design

```rust
/// Low-level JSON-RPC client for NEAR.
pub struct RpcClient {
    url: String,
    http: reqwest::Client,
    retry_config: RetryConfig,
}

impl RpcClient {
    pub fn new(url: impl Into<String>) -> Self;
    
    /// Make a raw RPC call.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<T, RpcError>;
    
    // High-level methods
    
    pub async fn view_account(
        &self,
        account_id: &AccountId,
        block: BlockReference,
    ) -> Result<AccountView, RpcError>;
    
    pub async fn view_access_key(
        &self,
        account_id: &AccountId,
        public_key: &PublicKey,
        block: BlockReference,
    ) -> Result<AccessKeyView, RpcError>;
    
    pub async fn call_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> Result<ViewFunctionResult, RpcError>;
    
    pub async fn send_transaction(
        &self,
        signed_tx: &SignedTransaction,
        wait_until: TxExecutionStatus,
    ) -> Result<FinalExecutionOutcome, RpcError>;
    
    pub async fn get_status(&self) -> Result<StatusResponse, RpcError>;
    
    pub async fn get_block(
        &self,
        block: BlockReference,
    ) -> Result<BlockView, RpcError>;
    
    pub async fn get_gas_price(
        &self,
        block_id: Option<&str>,
    ) -> Result<GasPrice, RpcError>;
}

#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 4,
            initial_delay_ms: 1000,
        }
    }
}
```

### Error Handling

```rust
#[derive(Debug, thiserror::Error)]
pub enum RpcError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    
    #[error("RPC error: {message} (code: {code})")]
    Rpc {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
    
    #[error("Account not found: {0}")]
    AccountNotFound(AccountId),
    
    #[error("Access key not found: {account_id} / {public_key}")]
    AccessKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
    },
    
    #[error("Contract execution failed: {message}")]
    ContractPanic { message: String },
    
    #[error("Invalid nonce: expected {expected}, got {actual}")]
    InvalidNonce { expected: u64, actual: u64 },
    
    #[error("Timeout after {0} retries")]
    Timeout(u32),
    
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}
```

---

## The Near Client

### Structure

```rust
/// The main client for interacting with NEAR Protocol.
#[derive(Clone)]
pub struct Near {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
}

impl Near {
    // ══════════════════════════════════════════════════════════════════
    // Construction
    // ══════════════════════════════════════════════════════════════════
    
    pub fn mainnet() -> NearBuilder {
        NearBuilder::new("https://rpc.mainnet.near.org")
    }
    
    pub fn testnet() -> NearBuilder {
        NearBuilder::new("https://rpc.testnet.near.org")
    }
    
    pub fn custom(rpc_url: impl Into<String>) -> NearBuilder {
        NearBuilder::new(rpc_url)
    }
    
    pub fn from_env() -> Result<Self, ConfigError> { ... }
    
    // ══════════════════════════════════════════════════════════════════
    // Read Operations
    // ══════════════════════════════════════════════════════════════════
    
    /// Get account balance.
    pub fn balance(&self, account_id: impl TryInto<AccountId>) -> BalanceQuery;
    
    /// Get full account info.
    pub fn account(&self, account_id: impl TryInto<AccountId>) -> AccountQuery;
    
    /// Check if account exists.
    pub fn account_exists(&self, account_id: impl TryInto<AccountId>) -> AccountExistsQuery;
    
    /// Call a view function.
    pub fn view<T: DeserializeOwned>(
        &self,
        contract_id: impl TryInto<AccountId>,
        method: &str,
    ) -> ViewCall<T>;
    
    /// List access keys for an account.
    pub fn access_keys(&self, account_id: impl TryInto<AccountId>) -> AccessKeysQuery;
    
    // ══════════════════════════════════════════════════════════════════
    // Write Operations
    // ══════════════════════════════════════════════════════════════════
    
    /// Transfer NEAR tokens.
    pub fn transfer(
        &self,
        receiver: impl TryInto<AccountId>,
        amount: impl TryInto<NearToken>,
    ) -> TransferCall;
    
    /// Call a change function on a contract.
    pub fn call(
        &self,
        contract_id: impl TryInto<AccountId>,
        method: &str,
    ) -> ContractCall;
    
    /// Create a batch transaction.
    pub fn batch(&self, receiver: impl TryInto<AccountId>) -> BatchBuilder;
    
    /// Create a new account.
    pub fn create_account(&self, new_account_id: impl TryInto<AccountId>) -> CreateAccountBuilder;
    
    /// Delete an account.
    pub fn delete_account(&self, account_id: impl TryInto<AccountId>) -> DeleteAccountBuilder;
    
    /// Add a key to an account.
    pub fn add_key(
        &self,
        account_id: impl TryInto<AccountId>,
        public_key: PublicKey,
    ) -> AddKeyBuilder;
    
    /// Delete a key from an account.
    pub fn delete_key(
        &self,
        account_id: impl TryInto<AccountId>,
        public_key: PublicKey,
    ) -> DeleteKeyCall;
    
    /// Deploy a contract.
    pub fn deploy(
        &self,
        account_id: impl TryInto<AccountId>,
        code: impl Into<Vec<u8>>,
    ) -> DeployBuilder;
    
    // ══════════════════════════════════════════════════════════════════
    // Token Helpers
    // ══════════════════════════════════════════════════════════════════
    
    /// Get a fungible token client.
    pub fn ft(&self, contract_id: impl TryInto<AccountId>) -> FungibleToken;
    
    /// Get a non-fungible token client.
    pub fn nft(&self, contract_id: impl TryInto<AccountId>) -> NonFungibleToken;
    
    // ══════════════════════════════════════════════════════════════════
    // Staking
    // ══════════════════════════════════════════════════════════════════
    
    /// Get a staking pool client.
    pub fn staking_pool(&self, pool_id: impl TryInto<AccountId>) -> StakingPool;
    
    // ══════════════════════════════════════════════════════════════════
    // Low-level Access
    // ══════════════════════════════════════════════════════════════════
    
    /// Get the underlying RPC client.
    pub fn rpc(&self) -> &RpcClient;
}

pub struct NearBuilder {
    rpc_url: String,
    signer: Option<Arc<dyn Signer>>,
    retry_config: RetryConfig,
}

impl NearBuilder {
    pub fn signer(self, signer: impl Signer + 'static) -> Self;
    pub fn retry_config(self, config: RetryConfig) -> Self;
    
    pub fn build(self) -> Near;
}

// Builder auto-converts to Near
impl From<NearBuilder> for Near {
    fn from(builder: NearBuilder) -> Self {
        builder.build()
    }
}
```

### Signer Trait

The `Signer` trait is the unified abstraction for signing transactions. It combines:
- **Account identity**: Which account is signing
- **Signing capability**: How to sign messages

The trait uses async signing to support remote signers (hardware wallets, cloud KMS, etc.).

```rust
/// Trait for signing transactions.
/// 
/// A signer knows which account it signs for and can sign arbitrary messages.
/// The `sign` method returns both the signature and the public key used,
/// which enables key rotation (where different calls may use different keys).
#[async_trait]
pub trait Signer: Send + Sync {
    /// The account this signer signs for.
    fn account_id(&self) -> &AccountId;
    
    /// Sign a message, returning the signature and the public key used.
    /// 
    /// Returning the public key allows signers to use different keys for
    /// different transactions (e.g., key rotation for high-throughput bots).
    async fn sign(&self, message: &[u8]) -> Result<(Signature, PublicKey), SignerError>;
}
```

### Signer Implementations

#### InMemorySigner

Simple signer with a single key stored in memory. Most common for scripts and bots.

```rust
pub struct InMemorySigner {
    account_id: AccountId,
    secret_key: SecretKey,
}

impl InMemorySigner {
    /// Create a new signer with an account ID and secret key.
    pub fn new(account_id: &str, secret_key: &str) -> Result<Self, Error>;
    
    /// Create from a SecretKey directly.
    pub fn from_secret_key(account_id: AccountId, secret_key: SecretKey) -> Self;
}

// Usage:
let signer = InMemorySigner::new("alice.testnet", "ed25519:...")?;
let near = Near::testnet().signer(signer).build();
```

#### FileSigner

Loads a key from `~/.near-credentials/{network}/{account}.json`. Compatible with near-cli.

```rust
pub struct FileSigner {
    account_id: AccountId,
    secret_key: SecretKey,  // Loaded at construction
}

impl FileSigner {
    /// Load credentials for an account from the standard NEAR credentials directory.
    pub fn new(network: &str, account_id: &str) -> Result<Self, Error>;
    
    /// Load from a specific file path.
    pub fn from_file(path: impl AsRef<Path>, account_id: &str) -> Result<Self, Error>;
}

// Usage:
let signer = FileSigner::new("testnet", "alice.testnet")?;
let near = Near::testnet().signer(signer).build();
```

#### EnvSigner

Loads credentials from environment variables. Useful for CI/CD and containers.

```rust
pub struct EnvSigner {
    account_id: AccountId,
    secret_key: SecretKey,
}

impl EnvSigner {
    /// Load from NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY environment variables.
    pub fn new() -> Result<Self, Error>;
    
    /// Load from custom environment variable names.
    pub fn from_env_vars(account_var: &str, key_var: &str) -> Result<Self, Error>;
}

// Usage (with NEAR_ACCOUNT_ID and NEAR_PRIVATE_KEY set):
let signer = EnvSigner::new()?;
let near = Near::testnet().signer(signer).build();
```

#### RotatingSigner

Uses multiple keys for the same account, rotating through them round-robin.
Solves the nonce collision problem for high-throughput applications.

```rust
pub struct RotatingSigner {
    account_id: AccountId,
    keys: Vec<SecretKey>,
    counter: AtomicUsize,
}

impl RotatingSigner {
    /// Create a rotating signer with multiple keys.
    pub fn new(account_id: &str, keys: Vec<SecretKey>) -> Result<Self, Error>;
    
    /// Parse keys from string format.
    pub fn from_key_strings(account_id: &str, keys: &[&str]) -> Result<Self, Error>;
}

// Usage:
let signer = RotatingSigner::from_key_strings("bot.near", &[
    "ed25519:key1...",
    "ed25519:key2...",
    "ed25519:key3...",
])?;
let near = Near::mainnet().signer(signer).build();

// Each concurrent transaction uses a different key, avoiding nonce collisions
futures::future::join_all((0..20).map(|_| {
    near.transfer("recipient.near", "0.1 NEAR")
})).await;
```

#### LedgerSigner (Future)

Hardware wallet support via Ledger.

```rust
pub struct LedgerSigner {
    account_id: AccountId,
    derivation_path: String,
}

impl LedgerSigner {
    pub fn new(account_id: &str, derivation_path: &str) -> Result<Self, Error>;
}

// The sign() method talks to the Ledger device
// User confirms transaction on the device
```

### Design Rationale

1. **No KeyStore trait**: The KeyStore abstraction added complexity without benefit. 
   Each signer implementation knows how to get its keys directly.

2. **Async signing**: Enables hardware wallets, remote KMS, and other async signing methods.

3. **Returns (Signature, PublicKey)**: Allows `RotatingSigner` to use different keys 
   for different transactions. The caller doesn't need to know which key was used upfront.

4. **Account ID on signer**: Every signer knows which account it signs for. This is 
   required information for building transactions anyway.

5. **Simple constructors**: `InMemorySigner::new("account", "key")` is easy to understand
   and use. No need for intermediate KeyStore objects.

---

## Query Builders

All queries implement `IntoFuture` so they can be `.await`ed directly:

```rust
pub struct BalanceQuery {
    near: Near,
    account_id: AccountId,
    block_ref: BlockReference,
}

impl BalanceQuery {
    pub fn at_block(mut self, height: u64) -> Self {
        self.block_ref = BlockReference::Height(height);
        self
    }
    
    pub fn at_block_hash(mut self, hash: CryptoHash) -> Self {
        self.block_ref = BlockReference::Hash(hash);
        self
    }
    
    pub fn finality(mut self, finality: Finality) -> Self {
        self.block_ref = BlockReference::Finality(finality);
        self
    }
}

impl IntoFuture for BalanceQuery {
    type Output = Result<AccountBalance, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;
    
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            let account = self.near.rpc.view_account(&self.account_id, self.block_ref).await?;
            Ok(AccountBalance {
                total: account.amount,
                available: account.amount.saturating_sub(account.locked),
                locked: account.locked,
                storage_usage: account.storage_usage,
            })
        })
    }
}

/// Simplified balance info.
pub struct AccountBalance {
    pub total: NearToken,
    pub available: NearToken,
    pub locked: NearToken,
    pub storage_usage: u64,
}

impl Display for AccountBalance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.available)
    }
}
```

---

## Transaction Builders

```rust
pub struct ContractCall {
    near: Near,
    contract_id: AccountId,
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl ContractCall {
    /// Set JSON arguments.
    pub fn args(mut self, args: impl Serialize) -> Self {
        self.args = serde_json::to_vec(&args).unwrap();
        self
    }
    
    /// Set Borsh arguments.
    pub fn args_borsh(mut self, args: impl BorshSerialize) -> Self {
        self.args = borsh::to_vec(&args).unwrap();
        self
    }
    
    /// Set raw byte arguments.
    pub fn args_raw(mut self, args: impl Into<Vec<u8>>) -> Self {
        self.args = args.into();
        self
    }
    
    /// Set gas limit.
    pub fn gas(mut self, gas: impl TryInto<Gas>) -> Self {
        self.gas = gas.try_into().unwrap_or(Gas::DEFAULT);
        self
    }
    
    /// Set attached deposit.
    pub fn deposit(mut self, amount: impl TryInto<NearToken>) -> Self {
        self.deposit = amount.try_into().unwrap_or(NearToken::ZERO);
        self
    }
    
    /// Override signer for this call.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self {
        self.signer_override = Some(Arc::new(signer));
        self
    }
    
    /// Set execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self {
        self.wait_until = status;
        self
    }
}

impl IntoFuture for ContractCall {
    type Output = Result<TransactionOutcome, Error>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;
    
    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            // 1. Get signer
            let signer = self.signer_override
                .as_ref()
                .or(self.near.signer.as_ref())
                .ok_or(Error::NoSigner)?;
            
            // 2. Get signer account ID
            let signer_id = signer.account_id()
                .or(self.near.default_account.as_ref())
                .ok_or(Error::NoSignerAccount)?
                .clone();
            
            // 3. Get access key (for nonce)
            let access_key = self.near.rpc.view_access_key(
                &signer_id,
                signer.public_key(),
                BlockReference::Finality(Finality::Optimistic),
            ).await?;
            
            // 4. Get recent block hash
            let block = self.near.rpc.get_block(BlockReference::Finality(Finality::Final)).await?;
            
            // 5. Build transaction
            let tx = Transaction {
                signer_id: signer_id.clone(),
                public_key: signer.public_key().clone(),
                nonce: access_key.nonce + 1,
                receiver_id: self.contract_id.clone(),
                block_hash: block.header.hash,
                actions: vec![
                    Action::FunctionCall {
                        method_name: self.method,
                        args: self.args,
                        gas: self.gas,
                        deposit: self.deposit,
                    }
                ],
            };
            
            // 6. Sign
            let signed_tx = tx.sign(signer.as_ref())?;
            
            // 7. Send
            let outcome = self.near.rpc.send_transaction(&signed_tx, self.wait_until).await?;
            
            Ok(TransactionOutcome::from(outcome))
        })
    }
}
```

---

## Transaction Builder (Multi-Action Transactions)

The `TransactionBuilder` allows chaining multiple actions into a single atomic transaction.
All actions either succeed together or fail together.

```rust
/// Create a transaction builder targeting a receiver account.
/// 
/// The receiver_id is the account that receives all actions in this transaction.
/// NEAR transactions are single-receiver: all actions go to the same account.
pub fn transaction(&self, receiver_id: impl AsRef<str>) -> TransactionBuilder;

pub struct TransactionBuilder {
    rpc: Arc<RpcClient>,
    signer: Option<Arc<dyn Signer>>,
    receiver_id: AccountId,
    actions: Vec<Action>,
    signer_override: Option<Arc<dyn Signer>>,
    wait_until: TxExecutionStatus,
}

impl TransactionBuilder {
    // ═══════════════════════════════════════════════════════════════════════
    // Action methods - each returns Self for chaining
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Add a create account action.
    pub fn create_account(mut self) -> Self;
    
    /// Add a transfer action (accepts "5 NEAR" strings).
    pub fn transfer(mut self, amount: impl AsRef<str>) -> Self;
    
    /// Add a deploy contract action.
    pub fn deploy(mut self, code: impl Into<Vec<u8>>) -> Self;
    
    /// Add a function call action (returns CallBuilder for args/gas/deposit).
    pub fn call(self, method: &str) -> CallBuilder;
    
    /// Add a full access key.
    pub fn add_full_access_key(mut self, public_key: PublicKey) -> Self;
    
    /// Add a function call access key.
    pub fn add_function_call_key(
        mut self,
        public_key: PublicKey,
        receiver_id: impl AsRef<str>,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self;
    
    /// Delete an access key.
    pub fn delete_key(mut self, public_key: PublicKey) -> Self;
    
    /// Delete the account (transfers remaining balance to beneficiary).
    pub fn delete_account(mut self, beneficiary_id: impl AsRef<str>) -> Self;
    
    /// Add a stake action.
    pub fn stake(mut self, amount: impl AsRef<str>, public_key: PublicKey) -> Self;
    
    // ═══════════════════════════════════════════════════════════════════════
    // Configuration
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Override the signer for this transaction.
    pub fn sign_with(mut self, signer: impl Signer + 'static) -> Self;
    
    /// Set the execution wait level.
    pub fn wait_until(mut self, status: TxExecutionStatus) -> Self;
    
    // ═══════════════════════════════════════════════════════════════════════
    // Execution
    // ═══════════════════════════════════════════════════════════════════════
    
    /// Send the transaction.
    pub fn send(self) -> TransactionSend;
}

// TransactionBuilder implements IntoFuture so it can be .await'd directly
impl IntoFuture for TransactionBuilder {
    type Output = Result<FinalExecutionOutcome, Error>;
}

/// Builder for configuring a function call within a transaction.
pub struct CallBuilder {
    builder: TransactionBuilder,
    method: String,
    args: Vec<u8>,
    gas: Gas,
    deposit: NearToken,
}

impl CallBuilder {
    /// Set JSON arguments.
    pub fn args<A: Serialize>(mut self, args: A) -> Self;
    
    /// Set Borsh arguments.
    pub fn args_borsh<A: BorshSerialize>(mut self, args: A) -> Self;
    
    /// Set raw byte arguments.
    pub fn args_raw(mut self, args: Vec<u8>) -> Self;
    
    /// Set gas limit (accepts "30 Tgas" strings).
    pub fn gas(mut self, gas: impl AsRef<str>) -> Self;
    
    /// Set attached deposit (accepts "1 NEAR" strings).
    pub fn deposit(mut self, amount: impl AsRef<str>) -> Self;
    
    // All TransactionBuilder methods are available for chaining:
    pub fn call(self, method: &str) -> CallBuilder;
    pub fn transfer(self, amount: impl AsRef<str>) -> TransactionBuilder;
    pub fn create_account(self) -> TransactionBuilder;
    // ... etc
    
    /// Send the transaction.
    pub fn send(self) -> TransactionSend;
}

// CallBuilder also implements IntoFuture
impl IntoFuture for CallBuilder {
    type Output = Result<FinalExecutionOutcome, Error>;
}
```

### Usage Examples

```rust
use near_kit::prelude::*;

// ═══════════════════════════════════════════════════════════════════════════
// Create a new sub-account with funding and a key
// ═══════════════════════════════════════════════════════════════════════════

near.transaction("new.alice.testnet")
    .create_account()
    .transfer("5 NEAR")
    .add_full_access_key(new_public_key)
    .send()
    .await?;

// ═══════════════════════════════════════════════════════════════════════════
// Deploy contract and call init method
// ═══════════════════════════════════════════════════════════════════════════

near.transaction("contract.alice.testnet")
    .create_account()
    .transfer("10 NEAR")
    .add_full_access_key(key)
    .deploy(wasm_code)
    .call("init")
        .args(json!({ "owner": "alice.testnet" }))
    .send()
    .await?;

// ═══════════════════════════════════════════════════════════════════════════
// Multiple function calls in one transaction
// ═══════════════════════════════════════════════════════════════════════════

near.transaction("defi.testnet")
    .call("deposit")
        .deposit("10 NEAR")
    .call("stake")
        .args(json!({ "amount": "10000000000000000000000000" }))
        .gas("100 Tgas")
    .send()
    .await?;

// ═══════════════════════════════════════════════════════════════════════════
// Delete an account
// ═══════════════════════════════════════════════════════════════════════════

near.transaction("old.alice.testnet")
    .delete_account("alice.testnet")  // beneficiary
    .send()
    .await?;
```

---

## Typed Contract Interfaces (Proc Macro)

The `#[near_kit::contract]` proc macro provides compile-time type safety for contract interactions. Instead of using stringly-typed method names and `serde_json::Value` for arguments, you define a Rust trait that mirrors the contract's interface.

### Why Typed Contracts?

**Without typed contracts:**
```rust
// Method names are strings - typos compile fine
let count: u64 = near.view("counter.near", "get_counnt").await?;  // typo!

// Args are JSON - wrong fields compile fine  
near.call("counter.near", "add")
    .args(json!({ "valuee": 5 }))  // typo!
    .await?;
```

**With typed contracts:**
```rust
#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;
    
    #[call]
    fn add(&mut self, args: AddArgs);
}

let counter = near.contract::<Counter>("counter.near");
let count = counter.get_count().await?;  // Compile-time checked
counter.add(AddArgs { value: 5 }).await?;  // Compile-time checked
```

### Basic Usage

```rust
use near_kit::prelude::*;
use serde::{Serialize, Deserialize};

// ══════════════════════════════════════════════════════════════════════════════
// Define the contract interface
// ══════════════════════════════════════════════════════════════════════════════

#[near_kit::contract]
pub trait Counter {
    // View methods: use &self (no state mutation)
    fn get_count(&self) -> u64;
    
    // Call methods: use &mut self + #[call] attribute
    #[call]
    fn increment(&mut self);
    
    // Call with arguments - use a single struct
    #[call]
    fn add(&mut self, args: AddArgs);
    
    // Payable call method
    #[call(payable)]
    fn donate(&mut self);
}

// Argument structs must implement Serialize (for JSON contracts)
#[derive(Serialize)]
pub struct AddArgs {
    pub value: u64,
}

// ══════════════════════════════════════════════════════════════════════════════
// Use the contract
// ══════════════════════════════════════════════════════════════════════════════

#[tokio::main]
async fn main() -> Result<(), near_kit::Error> {
    let near = Near::testnet()
        .signer(InMemorySigner::new("alice.testnet", "ed25519:...")?)
        .build();
    
    // Create a typed contract client
    let counter = near.contract::<Counter>("counter.testnet");
    
    // View calls - just await
    let count: u64 = counter.get_count().await?;
    println!("Current count: {}", count);
    
    // Change calls - just await
    counter.increment().await?;
    
    // Change calls with args
    counter.add(AddArgs { value: 5 }).await?;
    
    // Payable calls - chain .deposit() before await
    counter.donate().deposit("1 NEAR").await?;
    
    // Override gas for any call
    counter.add(AddArgs { value: 10 }).gas("50 Tgas").await?;
    
    Ok(())
}
```

### View vs Call Methods

The distinction between view and call methods is determined by `&self` vs `&mut self`:

| Receiver | Attribute | Method Type | Description |
|----------|-----------|-------------|-------------|
| `&self` | (none) | View | Read-only, no gas cost, no signer needed |
| `&mut self` | `#[call]` | Call | State change, requires gas and signer |
| `&mut self` | `#[call(payable)]` | Payable Call | Can receive NEAR deposit |

```rust
#[near_kit::contract]
pub trait MyContract {
    // View method - read only
    fn get_owner(&self) -> AccountId;
    
    // View method with args
    fn get_balance(&self, args: GetBalanceArgs) -> NearToken;
    
    // Call method - changes state
    #[call]
    fn set_owner(&mut self, args: SetOwnerArgs);
    
    // Payable call - can receive deposit
    #[call(payable)]
    fn purchase(&mut self, args: PurchaseArgs);
}
```

### Argument Structs

Contract methods that take arguments must use a **single struct** parameter. This struct is serialized to JSON (or Borsh) when calling the contract.

```rust
use serde::Serialize;

// For JSON contracts (default), args must implement Serialize
#[derive(Serialize)]
pub struct TransferArgs {
    pub receiver_id: AccountId,
    pub amount: NearToken,
}

// Optional fields work naturally with serde
#[derive(Serialize)]
pub struct MintArgs {
    pub token_id: String,
    pub metadata: Option<TokenMetadata>,
}

#[near_kit::contract]
pub trait FungibleToken {
    fn ft_balance_of(&self, args: FtBalanceArgs) -> NearToken;
    
    #[call(payable)]
    fn ft_transfer(&mut self, args: TransferArgs);
}

#[derive(Serialize)]
pub struct FtBalanceArgs {
    pub account_id: AccountId,
}
```

**Why single struct instead of multiple parameters?**
- Explicit field names in serialized JSON
- Matches how NEAR contracts define their parameters
- Easier to add optional fields
- No ambiguity about serialization order

### Return Types

Return types must implement `Deserialize` (for JSON) or `BorshDeserialize` (for Borsh contracts).

```rust
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TokenMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub media: Option<String>,
}

#[near_kit::contract]
pub trait NFT {
    // Simple return type
    fn nft_total_supply(&self) -> u64;
    
    // Complex return type
    fn nft_token(&self, args: TokenArgs) -> Option<Token>;
}

#[derive(Deserialize)]
pub struct Token {
    pub token_id: String,
    pub owner_id: AccountId,
    pub metadata: Option<TokenMetadata>,
}

#[derive(Serialize)]
pub struct TokenArgs {
    pub token_id: String,
}
```

### Borsh Serialization

Some NEAR contracts use Borsh instead of JSON for serialization. Use the `borsh` attribute:

```rust
use borsh::{BorshSerialize, BorshDeserialize};

#[near_kit::contract(borsh)]
pub trait BorshContract {
    fn get_data(&self) -> MyData;
    
    #[call]
    fn set_data(&mut self, args: SetDataArgs);
}

// Args must implement BorshSerialize
#[derive(BorshSerialize)]
pub struct SetDataArgs {
    pub key: String,
    pub value: Vec<u8>,
}

// Returns must implement BorshDeserialize
#[derive(BorshDeserialize)]
pub struct MyData {
    pub key: String,
    pub value: Vec<u8>,
}
```

The serialization format applies to all methods in the trait:

| Attribute | Args Requirement | Return Requirement |
|-----------|------------------|-------------------|
| `#[near_kit::contract]` | `Serialize` | `DeserializeOwned` |
| `#[near_kit::contract(json)]` | `Serialize` | `DeserializeOwned` |
| `#[near_kit::contract(borsh)]` | `BorshSerialize` | `BorshDeserialize` |

### Call Method Options

Call methods return a builder that allows configuring gas and deposit before execution:

```rust
#[near_kit::contract]
pub trait MyContract {
    #[call]
    fn do_something(&mut self, args: Args);
    
    #[call(payable)]
    fn buy_item(&mut self, args: BuyArgs);
}

let contract = near.contract::<MyContract>("contract.near");

// Default gas (30 Tgas), no deposit
contract.do_something(Args { ... }).await?;

// Custom gas
contract.do_something(Args { ... })
    .gas("100 Tgas")
    .await?;

// With deposit (only valid for payable methods)
contract.buy_item(BuyArgs { ... })
    .deposit("5 NEAR")
    .await?;

// Both gas and deposit
contract.buy_item(BuyArgs { ... })
    .gas("50 Tgas")
    .deposit("1 NEAR")
    .await?;

// Override signer for this call
contract.do_something(Args { ... })
    .sign_with(other_signer)
    .await?;

// Wait for specific finality
contract.do_something(Args { ... })
    .wait_until(TxExecutionStatus::Final)
    .await?;
```

### View Method Options

View methods can specify which block to query:

```rust
#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;
}

let counter = near.contract::<Counter>("counter.near");

// Default: final block
let count = counter.get_count().await?;

// Specific block height
let count = counter.get_count()
    .at_block(100_000_000)
    .await?;

// Specific finality
let count = counter.get_count()
    .finality(Finality::Optimistic)
    .await?;
```

### Complete Example: FT Contract

```rust
use near_kit::prelude::*;
use serde::{Serialize, Deserialize};

/// NEP-141 Fungible Token interface
#[near_kit::contract]
pub trait FungibleToken {
    // ─── View Methods ───────────────────────────────────────────────────────
    
    fn ft_total_supply(&self) -> NearToken;
    
    fn ft_balance_of(&self, args: FtBalanceArgs) -> NearToken;
    
    fn ft_metadata(&self) -> FtMetadata;
    
    // ─── Call Methods ───────────────────────────────────────────────────────
    
    #[call(payable)]
    fn ft_transfer(&mut self, args: FtTransferArgs);
    
    #[call(payable)]
    fn ft_transfer_call(&mut self, args: FtTransferCallArgs);
    
    #[call(payable)]
    fn storage_deposit(&mut self, args: StorageDepositArgs);
}

// ─── Argument Structs ─────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct FtBalanceArgs {
    pub account_id: AccountId,
}

#[derive(Serialize)]
pub struct FtTransferArgs {
    pub receiver_id: AccountId,
    pub amount: NearToken,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

#[derive(Serialize)]
pub struct FtTransferCallArgs {
    pub receiver_id: AccountId,
    pub amount: NearToken,
    pub msg: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memo: Option<String>,
}

#[derive(Serialize)]
pub struct StorageDepositArgs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<AccountId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registration_only: Option<bool>,
}

// ─── Return Types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct FtMetadata {
    pub spec: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub icon: Option<String>,
}

// ─── Usage ────────────────────────────────────────────────────────────────────

async fn example(near: &Near) -> Result<(), near_kit::Error> {
    let usdc = near.contract::<FungibleToken>("usdc.near");
    
    // Check balance
    let balance = usdc.ft_balance_of(FtBalanceArgs {
        account_id: "alice.near".parse()?,
    }).await?;
    
    println!("Balance: {}", balance);
    
    // Transfer tokens
    usdc.ft_transfer(FtTransferArgs {
        receiver_id: "bob.near".parse()?,
        amount: "100".parse()?, // Assuming token uses NearToken-like parsing
        memo: Some("Payment".to_string()),
    })
    .deposit("1 yocto")  // Required for ft_transfer
    .await?;
    
    Ok(())
}
```

---

### Proc Macro Implementation Details

This section provides implementation guidance for the `#[near_kit::contract]` proc macro.

#### Crate Structure

The proc macro requires a separate crate (Rust limitation):

```
near-kit-rs/
├── Cargo.toml                    # Main crate, depends on near-kit-macros
├── src/
│   └── ...
├── near-kit-macros/              # Proc macro crate
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs                # Proc macro implementation
```

**near-kit-macros/Cargo.toml:**
```toml
[package]
name = "near-kit-macros"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
syn = { version = "2", features = ["full", "parsing"] }
quote = "1"
proc-macro2 = "1"
```

**Main crate Cargo.toml addition:**
```toml
[dependencies]
near-kit-macros = { path = "./near-kit-macros" }
```

#### What the Macro Generates

For this input:

```rust
#[near_kit::contract]
pub trait Counter {
    fn get_count(&self) -> u64;
    
    #[call]
    fn increment(&mut self);
    
    #[call]
    fn add(&mut self, args: AddArgs);
    
    #[call(payable)]
    fn donate(&mut self);
}
```

The macro generates:

```rust
// 1. Keep the original trait (for documentation)
pub trait Counter {
    fn get_count(&self) -> u64;
    fn increment(&mut self);
    fn add(&mut self, args: AddArgs);
    fn donate(&mut self);
}

// 2. Generate a client struct
pub struct CounterClient<'a> {
    near: &'a Near,
    contract_id: AccountId,
}

// 3. Implement the Contract marker trait
impl near_kit::Contract for dyn Counter {
    type Client<'a> = CounterClient<'a>;
}

// 4. Implement methods on the client
impl<'a> CounterClient<'a> {
    pub fn new(near: &'a Near, contract_id: AccountId) -> Self {
        Self { near, contract_id }
    }
    
    // View method: returns ViewCall<T> which impls IntoFuture
    pub fn get_count(&self) -> ViewCall<'a, u64> {
        self.near.view::<u64>(&self.contract_id, "get_count")
    }
    
    // Call method (no args): returns ContractCall
    pub fn increment(&self) -> ContractCall<'a> {
        self.near.call(&self.contract_id, "increment")
    }
    
    // Call method (with args): returns ContractCall
    pub fn add(&self, args: AddArgs) -> ContractCall<'a> {
        self.near.call(&self.contract_id, "add")
            .args(args)  // Serializes using serde_json
    }
    
    // Payable call method: same as call, user chains .deposit()
    pub fn donate(&self) -> ContractCall<'a> {
        self.near.call(&self.contract_id, "donate")
    }
}
```

#### For Borsh Contracts

When `#[near_kit::contract(borsh)]` is used:

```rust
// Call method with borsh serialization
pub fn set_data(&self, args: SetDataArgs) -> ContractCall<'a> {
    self.near.call(&self.contract_id, "set_data")
        .args_borsh(args)  // Serializes using borsh
}

// View method with borsh deserialization
pub fn get_data(&self) -> ViewCallBorsh<'a, MyData> {
    self.near.view_borsh::<MyData>(&self.contract_id, "get_data")
}
```

#### Method Name Derivation

Method names map 1:1 from Rust to the contract:

| Rust Method | Contract Method |
|-------------|-----------------|
| `get_count` | `get_count` |
| `ft_balance_of` | `ft_balance_of` |
| `nft_transfer` | `nft_transfer` |

#### Parsing the Trait

The macro needs to parse:

1. **Trait attributes**: `#[near_kit::contract]` or `#[near_kit::contract(borsh)]`
2. **Method receiver**: `&self` (view) or `&mut self` (call)
3. **Method attributes**: `#[call]` or `#[call(payable)]`
4. **Method arguments**: Zero or one struct argument
5. **Return type**: The type to deserialize into

**Validation rules:**
- View methods (`&self`) must NOT have `#[call]` attribute
- Call methods (`&mut self`) MUST have `#[call]` or `#[call(payable)]` attribute
- Methods can have 0 or 1 argument (if 1, it must be a type, not multiple params)
- `payable` is only valid on `#[call]` methods

#### The Contract Trait and near.contract::<T>()

The main crate needs:

```rust
/// Marker trait for contract interfaces.
/// Implemented by the proc macro for each #[near_kit::contract] trait.
pub trait Contract {
    type Client<'a>;
}

impl Near {
    /// Create a typed contract client.
    pub fn contract<T: Contract + ?Sized>(&self, id: impl TryInto<AccountId>) -> T::Client<'_>
    where
        T::Client<'_>: ContractClient,
    {
        T::Client::new(self, id.try_into().expect("invalid account id"))
    }
}

/// Trait for contract client constructors.
pub trait ContractClient: Sized {
    fn new(near: &Near, contract_id: AccountId) -> Self;
}
```

#### Required Changes to Existing Types

The `ViewCall<T>` and `ContractCall` builders may need minor adjustments:

```rust
// ViewCall needs to support borsh deserialization
impl<'a, T> ViewCall<'a, T> {
    // Existing: JSON deserialization (default)
    pub async fn json(self) -> Result<T, Error>
    where
        T: DeserializeOwned;
    
    // New: Borsh deserialization
    pub async fn borsh(self) -> Result<T, Error>
    where
        T: BorshDeserialize;
}

// Or use a separate type for borsh views:
pub struct ViewCallBorsh<'a, T> { ... }
```

---

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum Error {
    // ─── Configuration ───
    #[error("No signer configured. Call .signer() on NearBuilder or .sign_with() on the operation.")]
    NoSigner,
    
    #[error("No signer account ID. Call .default_account() on NearBuilder or use a signer with an account ID.")]
    NoSignerAccount,
    
    #[error("Invalid configuration: {0}")]
    Config(String),
    
    // ─── Parsing ───
    #[error(transparent)]
    ParseAccountId(#[from] ParseAccountIdError),
    
    #[error(transparent)]
    ParseAmount(#[from] ParseAmountError),
    
    #[error(transparent)]
    ParseGas(#[from] ParseGasError),
    
    #[error(transparent)]
    ParseKey(#[from] ParseKeyError),
    
    // ─── RPC ───
    #[error(transparent)]
    Rpc(#[from] RpcError),
    
    // ─── Transaction ───
    #[error("Transaction failed: {0}")]
    TransactionFailed(String),
    
    #[error("Contract panic: {0}")]
    ContractPanic(String),
    
    // ─── Signing ───
    #[error("Signing failed: {0}")]
    Signing(#[from] SignerError),
    
    // ─── Serialization ───
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    
    #[error("Borsh error: {0}")]
    Borsh(String),
}
```

---

## Example Usage

Here's what the final API should look like:

```rust
use near_kit::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ══════════════════════════════════════════════════════════════════
    // Setup - Multiple ways to configure a signer
    // ══════════════════════════════════════════════════════════════════
    
    // Option 1: Direct key
    let near = Near::testnet()
        .signer(InMemorySigner::new("alice.testnet", "ed25519:...")?)
        .build();
    
    // Option 2: From file (~/.near-credentials/testnet/alice.testnet.json)
    let near = Near::testnet()
        .signer(FileSigner::new("testnet", "alice.testnet")?)
        .build();
    
    // Option 3: From environment variables
    let near = Near::testnet()
        .signer(EnvSigner::new()?)
        .build();
    
    // Option 4: Rotating keys for high-throughput
    let near = Near::mainnet()
        .signer(RotatingSigner::from_key_strings("bot.near", &[
            "ed25519:key1...",
            "ed25519:key2...",
        ])?)
        .build();
    
    // ══════════════════════════════════════════════════════════════════
    // Read Operations
    // ══════════════════════════════════════════════════════════════════
    
    // Check balance
    let balance = near.balance("alice.testnet").await?;
    println!("Balance: {}", balance);  // "10.5 NEAR"
    
    // View call
    let count: u64 = near.view("counter.testnet", "get_count").await?;
    println!("Count: {}", count);
    
    // View call with args
    let messages: Vec<Message> = near
        .view("guestbook.testnet", "get_messages")
        .args(json!({ "limit": 10 }))
        .await?;
    
    // Historical query
    let old_balance = near.balance("alice.testnet")
        .at_block(100_000_000)
        .await?;
    
    // ══════════════════════════════════════════════════════════════════
    // Write Operations
    // ══════════════════════════════════════════════════════════════════
    
    // Simple transfer
    near.transfer("bob.testnet", "1 NEAR").await?;
    
    // Contract call
    near.call("counter.testnet", "increment").await?;
    
    // Contract call with args, gas, deposit
    near.call("nft.testnet", "nft_mint")
        .args(json!({ "token_id": "1", "receiver_id": "alice.testnet" }))
        .gas("100 Tgas")
        .deposit("0.1 NEAR")
        .await?;
    
    // Wait for finality
    near.transfer("bob.testnet", "1000 NEAR")
        .wait_until(TxExecutionStatus::Final)
        .await?;
    
    // ══════════════════════════════════════════════════════════════════
    // Multi-Action Transactions
    // ══════════════════════════════════════════════════════════════════
    
    // Create account, fund it, deploy contract
    near.transaction("new.alice.testnet")
        .create_account()
        .transfer("5 NEAR")
        .add_full_access_key(new_public_key)
        .deploy(wasm_code)
        .call("init")
            .args(json!({ "owner": "alice.testnet" }))
        .send()
        .await?;
    
    // ══════════════════════════════════════════════════════════════════
    // FT/NFT
    // ══════════════════════════════════════════════════════════════════
    
    let usdc = near.ft("usdc.testnet");
    let balance = usdc.balance_of("alice.testnet").await?;
    usdc.transfer("bob.testnet", "100").await?;
    
    let nft = near.nft("nft.testnet");
    let tokens = nft.tokens_for_owner("alice.testnet").await?;
    nft.transfer("bob.testnet", "token-123").await?;
    
    // ══════════════════════════════════════════════════════════════════
    // Typed Contract Interfaces
    // ══════════════════════════════════════════════════════════════════
    
    // Define a typed interface for a contract
    #[near_kit::contract]
    pub trait Counter {
        fn get_count(&self) -> u64;
        
        #[call]
        fn increment(&mut self);
        
        #[call]
        fn add(&mut self, args: AddArgs);
    }
    
    #[derive(Serialize)]
    pub struct AddArgs { value: u64 }
    
    // Create typed client
    let counter = near.contract::<Counter>("counter.testnet");
    
    // Compile-time checked method calls
    let count = counter.get_count().await?;
    counter.increment().await?;
    counter.add(AddArgs { value: 5 }).await?;
    
    Ok(())
}
```

---

## Implementation Roadmap

### Phase 1: Core Types (Week 1)
- [ ] `AccountId` with validation
- [ ] `NearToken` with string parsing
- [ ] `Gas` with string parsing
- [ ] `PublicKey`, `SecretKey`, `Signature`
- [ ] `CryptoHash`
- [ ] `BlockReference`, `Finality`, `TxExecutionStatus`
- [ ] Basic error types

### Phase 2: RPC Client (Week 1-2)
- [ ] `RpcClient` with retry logic
- [ ] RPC response types (hand-rolled from actual responses)
- [ ] `view_account`, `view_access_key`, `call_function`
- [ ] `send_transaction` with `wait_until`
- [ ] `get_block`, `get_status`
- [ ] Error parsing from RPC responses

### Phase 3: Transaction Building (Week 2)
- [ ] `Transaction`, `SignedTransaction` types
- [ ] `Action` enum (all action types)
- [ ] Borsh serialization for transactions
- [ ] Transaction signing

### Phase 4: Near Client - Reads (Week 2-3)
- [ ] `Near` struct and `NearBuilder`
- [ ] `BalanceQuery` and `AccountBalance`
- [ ] `AccountQuery`
- [ ] `ViewCall<T>` with args variants

### Phase 5: Near Client - Writes (Week 3)
- [ ] `SecretKeySigner`
- [ ] `TransferCall`
- [ ] `ContractCall` with args, gas, deposit
- [ ] `TransactionOutcome`

### Phase 6: Batch Transactions (Week 3-4)
- [ ] `BatchBuilder`
- [ ] `BatchCallBuilder`
- [ ] All action methods

### Phase 7: Helpers (Week 4)
- [ ] `FungibleToken` (FT operations)
- [ ] `NonFungibleToken` (NFT operations)
- [ ] `StakingPool`
- [ ] Account management (`create_account`, `delete_account`, `add_key`, `delete_key`)
- [ ] `DeployBuilder`

### Phase 8: Typed Contract Interfaces (Week 5)
- [ ] Create `near-kit-macros` proc macro crate
- [ ] Implement `#[near_kit::contract]` attribute macro
- [ ] Parse trait definitions (methods, receivers, attributes)
- [ ] Generate client struct with typed methods
- [ ] Support `#[call]` and `#[call(payable)]` attributes
- [ ] Support `#[near_kit::contract(borsh)]` for Borsh serialization
- [ ] Add `Contract` marker trait and `near.contract::<T>()` method
- [ ] Add `ViewCallBorsh<T>` for Borsh view deserialization
- [ ] Unit tests for macro expansion
- [ ] Integration tests with real contracts

### Phase 9: Polish (Week 5-6)
- [ ] Comprehensive error messages
- [ ] Documentation
- [ ] Examples
- [ ] Tests (unit + integration)

---

## Testing Strategy

### Unit Tests
- Type parsing (`NearToken::from_str`, `Gas::from_str`, `AccountId::from_str`)
- Serialization/deserialization
- Transaction building

### Integration Tests
- Against NEAR testnet
- Full transaction flows
- Error handling

### Use near-sandbox for isolated testing:
```rust
#[tokio::test]
async fn test_transfer() {
    let sandbox = near_sandbox::Sandbox::start().await.unwrap();
    let near = Near::custom(sandbox.rpc_url())
        .signer(sandbox.root_signer())
        .build();
    
    // Create test accounts, perform operations, verify results
}
```

---

## Notes for Implementer

1. **Start with types**: Get `AccountId`, `NearToken`, `Gas` right first. Everything builds on these.

2. **RPC responses are the source of truth**: Capture actual RPC responses and derive types from them. Don't guess.

3. **Test string parsing thoroughly**: Users will pass `"5 NEAR"`, `"5near"`, `"5 near"`, `"5.5 NEAR"`, etc.

4. **Error messages matter**: When something fails, the error should tell the user exactly what went wrong and how to fix it.

5. **Use `impl TryInto<T>` for flexibility**: This allows accepting both `&str` and `AccountId` for account parameters.

6. **The near-kit TypeScript source is your friend**: When in doubt, check how near-kit handles it.

7. **Don't over-engineer v1**: Start simple, ship something that works for common cases, iterate.

---

## Sandbox Testing

The `sandbox` module (behind the `sandbox` feature flag) provides ergonomic APIs for testing against a local NEAR sandbox.

### Design Principles

1. **Consistent with production**: The code you write for tests should look like production code. `Near` is always the entry point - you just pass a different network config.

2. **Sandbox lifecycle management**: The library handles starting/stopping sandboxes, including a shared singleton for test performance.

3. **No magic account creation**: All accounts are created through the standard `near.transaction().create_account()` flow. No hidden state manipulation.

### API Overview

```rust
use near_kit::prelude::*;
use near_kit::sandbox::SandboxConfig;

// ══════════════════════════════════════════════════════════════════════════════
// Production code
// ══════════════════════════════════════════════════════════════════════════════

let near = Near::testnet().build();
let near = Near::mainnet().build();
let near = Near::custom("https://my-rpc.example.com").build();

// ══════════════════════════════════════════════════════════════════════════════
// Test code - same pattern, just different network
// ══════════════════════════════════════════════════════════════════════════════

// Shared sandbox (singleton) - fast, reuses across tests
let near = Near::sandbox(SandboxConfig::shared().await);

// Fresh sandbox - isolated, stopped on drop
let sandbox = SandboxConfig::fresh().await;
let near = Near::sandbox(&sandbox);

// Custom version
let sandbox = SandboxConfig::builder()
    .version("2.10.5")
    .fresh()
    .await;
let near = Near::sandbox(&sandbox);
```

### Shared vs Fresh Sandboxes

**Shared (singleton):**
- First call starts the sandbox, subsequent calls reuse it
- Persists for entire test run
- Fast - no startup cost after first test
- Tests share blockchain state (use unique account names)

**Fresh (owned):**
- New sandbox per call
- Stopped and cleaned up when dropped
- Slower - full startup cost each time
- Completely isolated state

### Convenience Method

Both `SharedSandbox` and `OwnedSandbox` provide a `.client()` shortcut:

```rust
// These are equivalent:
let near = Near::sandbox(SandboxConfig::shared().await);
let near = SandboxConfig::shared().await.client();

// For fresh sandboxes, keep a reference if you need it:
let sandbox = SandboxConfig::fresh().await;
let near = sandbox.client();
// sandbox stays alive, near can be used
```

### Full Test Example

```rust
use near_kit::prelude::*;
use near_kit::sandbox::SandboxConfig;

#[tokio::test]
async fn test_create_account_and_transfer() {
    // Get sandbox client (shared singleton)
    let near = Near::sandbox(SandboxConfig::shared().await);
    
    // Create a new account - same code as production!
    let alice_key = SecretKey::generate_ed25519();
    near.transaction("alice.sandbox")
        .create_account()
        .transfer("100 NEAR")
        .add_full_access_key(alice_key.public_key())
        .send()
        .wait_until(TxExecutionStatus::Final)
        .await
        .unwrap();
    
    // Verify
    let balance = near.balance("alice.sandbox").await.unwrap();
    assert!(balance.total > NearToken::from_near(99));
    
    // Use alice's account
    let alice_near = Near::custom(near.rpc_url())
        .credentials(alice_key.to_string(), "alice.sandbox")
        .unwrap()
        .build();
    
    alice_near.transfer("bob.sandbox", "10 NEAR").await.unwrap();
}
```

### Module Structure

```
near_kit::sandbox
├── SandboxConfig        # Factory for sandbox instances
│   ├── ::shared()       # Get global singleton
│   ├── ::fresh()        # Spawn new instance
│   └── ::builder()      # Builder for custom config
├── SharedSandbox        # Singleton wrapper, implements SandboxNetwork
├── OwnedSandbox         # Owned wrapper, stopped on drop
├── SandboxBuilder       # Builder for version and future options
└── (re-exports)
    ├── SandboxNetwork   # Trait for sandbox network config
    ├── ROOT_ACCOUNT     # "sandbox" constant
    └── ROOT_SECRET_KEY  # Root account secret key
```

### Feature Flag

The sandbox module requires the `sandbox` feature:

```toml
[dependencies]
near-kit = { version = "0.1", features = ["sandbox"] }
```

This keeps the main library lightweight for production use.

---

## Resources

- **near-kit (TypeScript reference)**: `/home/ricky/near-kit`
- **NEAR RPC docs**: https://docs.near.org/api/rpc/introduction
- **NEAR Protocol spec**: https://nomicon.io/
- **Borsh spec**: https://borsh.io/
