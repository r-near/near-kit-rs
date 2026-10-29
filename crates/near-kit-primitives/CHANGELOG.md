# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
