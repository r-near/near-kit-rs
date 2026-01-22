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

This is a single crate for simplicity. Proc macros (if needed) would be the only separate crate.

```
near-kit-rs/
├── Cargo.toml              # Single crate
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
│   └── client/
│       ├── mod.rs          # Re-exports client components
│       ├── near.rs         # Near client and NearBuilder
│       ├── rpc.rs          # RpcClient with retry logic
│       └── signer.rs       # Signer trait and SecretKeySigner
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

### Phase 8: Polish (Week 4-5)
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
