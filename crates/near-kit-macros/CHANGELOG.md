# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.3.0...near-kit-macros-v0.3.1) - 2026-02-03

### Other

- update to Rust 2024 edition and resolver v3 ([#15](https://github.com/r-near/near-kit-rs/pull/15))

## [0.3.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.2.0...near-kit-macros-v0.3.0) - 2026-01-30

### Added

- add Near::from_env() and ViewCall::borsh() ([#13](https://github.com/r-near/near-kit-rs/pull/13))

## [0.2.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.1.0...near-kit-macros-v0.2.0) - 2026-01-24

### Other

- add compile-fail tests for near-kit-macros
- consolidate transaction builders into TransactionBuilder
- release v0.1.0 ([#8](https://github.com/r-near/near-kit-rs/pull/8))

## [0.1.0](https://github.com/r-near/near-kit-rs/releases/tag/near-kit-macros-v0.1.0) - 2026-01-24

### Added

- add per-method serialization format override

### Fixed

- add allow(dead_code) to macro-generated trait

### Other

- remove prelude module, use direct imports
- convert to Cargo workspace structure
