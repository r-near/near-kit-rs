# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.2.0...near-kit-v0.3.0) - 2026-01-30

### Added

- add Near::from_env() and ViewCall::borsh() ([#13](https://github.com/r-near/near-kit-rs/pull/13))
- add known token constants with network-aware resolution

### Fixed

- use ak_nonce from InvalidNonce error for smarter retry
- add claim_key() for atomic key claiming in RotatingSigner

### Other

- simplify Signer trait to minimal key() interface
- add readme path for crates.io

## [0.2.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.1.0...near-kit-v0.2.0) - 2026-01-24

### Added

- add KeyPair type and runnable examples
- add offline signing support for air-gapped workflows
- add KeyringSigner for system keyring integration
- add token helpers for FT (NEP-141) and NFT (NEP-171)
- add seed phrase / mnemonic support for key derivation

### Fixed

- *(types)* validate public keys are on the curve when parsing
- *(types)* fix available balance calculation to account for storage ([#10](https://github.com/r-near/near-kit-rs/pull/10))
- correct test expectation for access_keys on non-existent accounts
- panic on invalid gas/deposit parsing instead of silent failure

### Other

- improve rustdoc with comprehensive guides and fix warnings
- add comprehensive unit tests for core types and client modules
- add integration tests for macro features
- add transaction failure integration tests
- add comprehensive integration tests for error paths
- remove unit tests from error.rs and rpc.rs
- add comprehensive integration tests for error handling
- add comprehensive unit tests for error.rs and client/rpc.rs
- use thiserror for DecodeError
- add AccountId::parse_lenient and remove dead code
- consolidate transaction builders into TransactionBuilder
- remove unused KeyPairProperties and AccountKeyPair structs

## [0.1.0](https://github.com/r-near/near-kit-rs/releases/tag/near-kit-v0.1.0) - 2026-01-24

### Added

- add NEP-413 message signing support
- add sandbox state patching for testing
- implement NEP-616 deterministic state init with proper account derivation
- add per-method serialization format override

### Fixed

- improve sandbox set_balance using raw RPC response
- add delay after sandbox_patch_state for race condition
- add allow(dead_code) to macro-generated trait
- allow dead_code for test trait used by macro
- add explicit lifetime annotations to SignFuture return types
- gate integration tests behind sandbox feature flag

### Other

- move integration tests into integration/ directory
- remove prelude module, use direct imports
- convert to Cargo workspace structure
