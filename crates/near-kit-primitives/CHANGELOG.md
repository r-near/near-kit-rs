# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.21.0](https://github.com/r-near/near-kit-rs/compare/near-kit-primitives-v0.20.1...near-kit-primitives-v0.21.0) - 2026-10-29

### Fixed

- classify 4xx JSON-RPC errors by code and status, keep URL secrets out of Debug, and publish only from the release PR ([#382](https://github.com/r-near/near-kit-rs/pull/382))
- [**breaking**] decode RPC outcomes as nearcore sends them, and retry only transient errors ([#376](https://github.com/r-near/near-kit-rs/pull/376))
- [**breaking**] read and write keys in the forms nearcore, near-cli-rs and near-api-js use ([#374](https://github.com/r-near/near-kit-rs/pull/374))

### Other

- [**breaking**] trim the public API before 0.30.0-rc.1, and fix what new users trip on ([#383](https://github.com/r-near/near-kit-rs/pull/383))
- [**breaking**] cut 10k lines of duplicated code, tests and docs ([#381](https://github.com/r-near/near-kit-rs/pull/381))
- rewrite the READMEs, crate front pages and MIGRATION.md for 0.30 ([#380](https://github.com/r-near/near-kit-rs/pull/380))
- rewrite the examples as runnable tasks and run them in CI ([#379](https://github.com/r-near/near-kit-rs/pull/379))
- document every public item and build the docs with -D warnings ([#377](https://github.com/r-near/near-kit-rs/pull/377))

This is the first supported release of near-kit-primitives. It holds
near-kit's protocol types, keys, signers, the `Viewer`/`TxSender` traits and
(with `contracts`) `#[contract]`, with no RPC client, async runtime or HTTP
stack. near-kit re-exports every item at the same `near_kit::` path, so
depend on this crate directly only where a network stack can't go, such as a
contract crate.

0.20.1 was published by mistake from a commit in the middle of the 0.30
refactor ([#372](https://github.com/r-near/near-kit-rs/pull/372)). Don't
depend on it. Everything below changed relative to near-kit 0.20.1's types;
see [MIGRATION.md](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#protocol-types).

- Moved here from near-kit ([#366](https://github.com/r-near/near-kit-rs/pull/366)), with client-free error types ([#365](https://github.com/r-near/near-kit-rs/pull/365)) and the 0.30 names ([#371](https://github.com/r-near/near-kit-rs/pull/371)).
- `#[contract]` bindings build with this crate alone ([#367](https://github.com/r-near/near-kit-rs/pull/367)).
- [**breaking**] Keys read and write in the forms nearcore, near-cli-rs and near-api-js use. Adds `ParseKeyError::KeyPairMismatch` ([#374](https://github.com/r-near/near-kit-rs/pull/374)).
- [**breaking**] Outcomes decode as nearcore sends them. Adds `ExecutionStatus::InvalidTx` ([#376](https://github.com/r-near/near-kit-rs/pull/376)).
- [**breaking**] `ValidatorStakeView` is now the stake struct itself. Removed helpers include `CryptoHash::to_vec`, `ChainId`'s `FromStr`/`AsRef<str>`, the `Signature::*_from_bytes` constructors, `TryFrom<u8> for KeyType`, `Transaction::get_hash_and_size` and the `AccessKeyPermission`/`AccessKey` constructors ([#381](https://github.com/r-near/near-kit-rs/pull/381)).
