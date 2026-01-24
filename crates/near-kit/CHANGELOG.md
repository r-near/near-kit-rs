# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0](https://github.com/r-near/near-kit-rs/releases/tag/near-kit-v0.1.0) - 2026-01-24

### Added

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
