# Agent Instructions for near-kit-rs

This document provides context and a task checklist for AI agents working on this project.

## Project Overview

**near-kit-rs** is a clean, ergonomic Rust client for NEAR Protocol. It's a ground-up implementation (not a refactor of `near-api-rs`) with hand-rolled types based on actual NEAR RPC responses.

### Design Philosophy

1. **Single entry point** - Everything flows through the `Near` client
2. **Configure once** - Network and signer set at client creation
3. **Type-safe but ergonomic** - Accept `"5 NEAR"` strings while maintaining type safety
4. **Explicit units** - No ambiguous amounts; must specify `NEAR`, `yocto`, `Tgas`, etc.
5. **Minimal dependencies** - No NEAR official crates; we hand-roll everything

### Key Patterns

- **String parsing**: `NearToken`, `Gas`, and `AccountId` all implement `FromStr` for human-readable input
- **IntoFuture**: Query builders implement `IntoFuture` so they can be `.await`ed directly
- **Builder pattern**: `NearBuilder` for client configuration, fluent builders for transactions
- **Block references**: Every query can specify a `BlockReference` (height, hash, or finality)

## Project Structure

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
├── SPEC.md                 # Full specification (READ THIS FIRST)
├── REFERENCE.md            # TypeScript patterns reference
└── AGENTS.md               # This file
```

## Reference Implementation

The TypeScript library at `/home/ricky/near-kit` is the primary design reference. Key files:

| File | Purpose |
|------|---------|
| `src/core/near.ts` | Main `Near` class |
| `src/core/rpc/rpc.ts` | RPC client with retry logic |
| `src/core/transaction.ts` | Transaction builder |
| `src/core/actions.ts` | Action factory functions |
| `src/utils/amount.ts` | NEAR amount parsing |
| `src/utils/validation.ts` | Gas parsing |

## Implementation Checklist

Use this checklist to track progress. Mark items with `[x]` when complete.

### Phase 1: Core Types ✅

- [x] `AccountId` - Validated NEAR account identifier
- [x] `NearToken` - Amount with yoctoNEAR precision, string parsing
- [x] `Gas` - Gas units with Tgas/Ggas parsing
- [x] `CryptoHash` - 32-byte SHA-256 hash
- [x] `PublicKey`, `SecretKey`, `Signature` - Ed25519 keys
- [x] `KeyType` - Ed25519 / Secp256k1 enum
- [x] `BlockReference`, `Finality`, `TxExecutionStatus`
- [x] `Action` enum (all action types)
- [x] `Transaction`, `SignedTransaction`
- [x] Error types (`ParseAccountIdError`, `ParseAmountError`, etc.)

### Phase 2: RPC Client ✅

- [x] `RpcClient` with retry logic and exponential backoff
- [x] JSON-RPC request/response handling
- [x] `view_account` - Get account info
- [x] `view_access_key` - Get access key info
- [x] `view_access_key_list` - List all access keys
- [x] `view_function` - Call view function
- [x] `block` - Get block info
- [x] `status` - Get node status
- [x] `gas_price` - Get gas price
- [x] `send_tx` - Send transaction
- [x] `tx_status` - Get transaction status
- [x] RPC error parsing (AccountNotFound, AccessKeyNotFound, etc.)

### Phase 3: Near Client ✅

- [x] `Near` struct - Main client
- [x] `NearBuilder` - Fluent configuration
- [x] `Signer` trait (async, returns Signature + PublicKey)
- [x] `InMemorySigner` - Single key in memory
- [x] `FileSigner` - Load from ~/.near-credentials
- [x] `EnvSigner` - Load from environment variables
- [x] `RotatingSigner` - Multiple keys, round-robin rotation
- [x] Network presets (mainnet, testnet)
- [x] `balance()` - Get account balance
- [x] `account()` - Get full account info
- [x] `account_exists()` - Check if account exists
- [x] `view()` / `view_with_args()` - View function calls
- [x] `access_keys()` - List access keys
- [x] `transfer()` - Transfer NEAR
- [x] `call()` / `call_with_args()` / `call_with_options()` - Function calls
- [x] `deploy()` - Deploy contract
- [x] `add_full_access_key()` / `delete_key()` - Key management

### Phase 4: Query Builders ✅

- [x] `BalanceQuery` with `IntoFuture`
- [x] `AccountQuery` with `IntoFuture`
- [x] `ViewCall<T>` with args variants and `IntoFuture`
- [x] `AccessKeysQuery` with `IntoFuture`
- [x] Block reference builder methods (`.at_block()`, `.finality()`)

### Phase 5: Transaction Builders ✅

- [x] `ContractCall` builder with `.args()`, `.gas()`, `.deposit()`
- [x] `TransferCall` builder with `.wait_until()`
- [x] `.sign_with()` for signer override

### Phase 6: Multi-Action Transactions ✅

- [x] `TransactionBuilder` - Fluent multi-action transaction API
- [x] `CallBuilder` - Function call within transactions
- [x] `.create_account()`, `.transfer()`, `.deploy()`, etc.
- [x] `.add_full_access_key()`, `.add_function_call_key()`
- [x] `.delete_key()`, `.delete_account()`, `.stake()`
- [x] `.send()` to execute transaction

### Phase 7: Token Helpers (TODO)

- [ ] `FungibleToken` - FT operations (NEP-141)
- [ ] `NonFungibleToken` - NFT operations (NEP-171)
- [ ] `ft().balance_of()`, `ft().transfer()`
- [ ] `nft().tokens_for_owner()`, `nft().transfer()`

### Phase 8: Typed Contract Interfaces (TODO)

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

### Phase 9: Advanced Features (TODO)

- [ ] `StakingPool` - Staking operations
- [ ] Seed phrase / mnemonic support in `SecretKeySigner`
- [ ] Environment-based configuration (`Near::from_env()`)
- [ ] Meta-transactions (delegate actions)

### Phase 10: Polish (TODO)

- [ ] Comprehensive error messages
- [ ] Full documentation with examples
- [ ] Example programs in `examples/`
- [ ] Integration tests against testnet
- [ ] CI/CD setup

## Coding Guidelines

### Committing Work

**Always commit your work when a task or feature is complete.** Use semantic commit messages:

- `feat: add query builders with IntoFuture support`
- `fix: handle UNKNOWN_ACCOUNT RPC errors correctly`
- `refactor: simplify Signer to use KeyStore`
- `test: add testnet integration tests`
- `docs: update AGENTS.md checklist`

Run `cargo fmt && cargo clippy && cargo test` before committing to ensure all checks pass.

### Conventions

1. **Use `impl AsRef<str>` or `impl TryInto<T>`** for string parameters that will be parsed
2. **Implement `FromStr`** for any type that can be parsed from a string
3. **Use `thiserror`** for error types
4. **Use `borsh`** for binary serialization (NEAR's format)
5. **Use `serde`** for JSON serialization (RPC)
6. **Fully qualify trait methods** when both `serde::Deserialize` and `borsh::BorshDeserialize` are in scope

### Development Setup

After cloning, install git hooks:
```bash
lefthook install
```

This sets up pre-commit hooks for:
- **rustfmt** - Code formatting (auto-fixes and stages)
- **clippy** - Linting with warnings as errors

And pre-push hooks for:
- **cargo test** - Run all tests

### Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture

# Run only sandbox integration tests
cargo test --features sandbox --test sandbox_integration
```

### Code Quality

Before committing, ensure:
```bash
cargo fmt          # Format code
cargo clippy       # Run linter  
cargo test         # Run tests
```

All checks must pass - the pre-commit hook enforces this.

### Key Files to Reference

When implementing new features, reference:

1. **SPEC.md** - Full API specification with code examples
2. **REFERENCE.md** - TypeScript patterns to emulate
3. **Existing implementations** - Follow patterns in `src/types/` and `src/client/`

## Resources

- **NEAR RPC docs**: https://docs.near.org/api/rpc/introduction
- **NEAR Protocol spec**: https://nomicon.io/
- **Borsh spec**: https://borsh.io/
- **TypeScript reference**: `/home/ricky/near-kit`
