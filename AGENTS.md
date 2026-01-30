# Agent Instructions for near-kit-rs

A clean, ergonomic Rust client for NEAR Protocol. Ground-up implementation with hand-rolled types based on actual NEAR RPC responses.

## Build, Lint, Test Commands

```bash
# Build
cargo build                     # Build all crates
cargo build -p near-kit         # Build main crate only

# Lint & Format
cargo fmt                       # Format code
cargo clippy                    # Run linter (warnings as errors in CI)
cargo clippy --all-features     # Lint with all features enabled

# Test
cargo test                      # Run all tests
cargo test test_name            # Run single test by name
cargo test test_name -- --exact # Run exact test match
cargo test -- --nocapture       # Show println! output
cargo test -p near-kit          # Test main crate only

# Sandbox integration tests (requires near-sandbox)
cargo test --features sandbox --test sandbox_integration
```

## Project Structure

```
near-kit-rs/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── near-kit/               # Main library crate
│   │   ├── src/
│   │   │   ├── lib.rs          # Public API exports
│   │   │   ├── error.rs        # Error types (thiserror)
│   │   │   ├── client/         # Near client, RPC, signers
│   │   │   ├── types/          # AccountId, NearToken, Gas, keys, etc.
│   │   │   ├── tokens/         # FT/NFT helpers (NEP-141, NEP-171)
│   │   │   └── contract.rs     # Typed contract support
│   │   └── examples/
│   └── near-kit-macros/        # Proc macros (#[near_kit::contract])
└── lefthook.yml                # Git hooks (clippy, fmt)
```

## Code Style Guidelines

### Imports

Order imports in groups separated by blank lines:
1. Standard library (`std::`)
2. External crates (alphabetical)
3. Crate-local imports (`crate::`, `super::`)

```rust
use std::fmt::{self, Display};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::error::ParseAmountError;
use crate::types::AccountId;
```

### Formatting

- Use `rustfmt` defaults (run `cargo fmt`)
- Max line width: 100 characters
- Use trailing commas in multi-line constructs
- Prefer explicit lifetimes over elision when it improves clarity

### Types and Naming

- **Structs**: PascalCase (`NearToken`, `AccountId`, `RpcClient`)
- **Traits**: PascalCase, often verbs or adjectives (`Signer`, `Contract`)
- **Functions/methods**: snake_case (`view_account`, `from_yoctonear`)
- **Constants**: SCREAMING_SNAKE_CASE (`YOCTO_PER_NEAR`, `ONE_TGAS`)
- **Type aliases**: PascalCase
- **Modules**: snake_case

### Error Handling

- Use `thiserror` for all error types
- Implement `From` conversions for error composition
- Provide helpful error messages with context
- Group errors by category (Parse*, Rpc*, Signer*, etc.)

```rust
#[derive(Debug, Error)]
pub enum ParseAmountError {
    #[error("Ambiguous amount '{0}'. Use explicit units like '5 NEAR' or '1000 yocto'")]
    AmbiguousAmount(String),

    #[error("Invalid amount format: '{0}'")]
    InvalidFormat(String),
}
```

### Documentation

- All public items must have doc comments
- Use `///` for item docs, `//!` for module docs
- Include examples in doc comments using ```` ```rust ````
- Use `# Example` sections for longer examples

### Key Patterns

1. **String parsing**: Implement `FromStr` for types that can be parsed from strings
2. **Builder pattern**: Use for complex configuration (`NearBuilder`, `ContractCall`)
3. **IntoFuture**: Query builders implement `IntoFuture` for direct `.await`
4. **Explicit units**: No bare numbers for amounts—require `NearToken::near(5)` or `"5 NEAR"`

```rust
// Good: explicit units
let amount = NearToken::near(5);
let gas = Gas::tgas(30);

// Also good: string parsing for runtime input
let amount: NearToken = "5 NEAR".parse()?;
```

### Serialization

- Use `serde` for JSON (RPC communication)
- Use `borsh` for binary (NEAR's on-chain format)
- When both traits are in scope, fully qualify: `serde::Deserialize::deserialize(...)`

### Feature Flags

- `sandbox`: Local testing with near-sandbox
- `keyring`: System keyring support (macOS Keychain, etc.)

## Git Hooks (Lefthook)

After cloning, run `bunx lefthook install` to set up hooks:

- **pre-commit**: Runs `cargo clippy --fix` and `cargo fmt` (auto-stages fixes)

## Commit Messages

Use conventional commits:

```
feat: add FT transfer_call support
fix: handle UNKNOWN_ACCOUNT RPC errors correctly  
refactor: simplify Signer trait
test: add sandbox integration tests
docs: update README examples
```

## Design Philosophy

1. **Single entry point**: Everything flows through the `Near` client
2. **Configure once**: Network and signer set at client creation
3. **Type-safe but ergonomic**: Accept both typed values and string parsing
4. **Explicit units**: Must specify `NEAR`, `yocto`, `Tgas`, etc.
5. **Minimal dependencies**: No NEAR official crates—hand-roll everything

## Dependencies Policy

**Allowed**: tokio, reqwest, serde, borsh, thiserror, ed25519-dalek, sha2, bs58, base64

**Not allowed**: near-primitives, near-crypto, near-jsonrpc-client (we hand-roll types)
