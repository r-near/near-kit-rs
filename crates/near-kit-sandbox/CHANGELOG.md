# Changelog

All notable changes to `near-kit-sandbox` are documented in this file.

## [Unreleased]

## [0.21.0](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.20.1...near-kit-sandbox-v0.21.0) - 2026-10-29

### Added

- [**breaking**] make outcomes must_use and add the helpers a relayer needs ([#372](https://github.com/r-near/near-kit-rs/pull/372))
- [**breaking**] rebuild the 0.30 API on two traits, Viewer and TxSender ([#368](https://github.com/r-near/near-kit-rs/pull/368))

### Fixed

- show only the RPC URL's scheme and host in Debug and tracing, and don't block or panic on WASI and sandbox failures ([#385](https://github.com/r-near/near-kit-rs/pull/385))
- classify 4xx JSON-RPC errors by code and status, keep URL secrets out of Debug, and publish only from the release PR ([#382](https://github.com/r-near/near-kit-rs/pull/382))
- [**breaking**] decode RPC outcomes as nearcore sends them, and retry only transient errors ([#376](https://github.com/r-near/near-kit-rs/pull/376))
- [**breaking**] share one nonce sequence per chain, account and key ([#373](https://github.com/r-near/near-kit-rs/pull/373))

### Other

- [**breaking**] trim the public API before 0.30.0-rc.1, and fix what new users trip on ([#383](https://github.com/r-near/near-kit-rs/pull/383))
- [**breaking**] cut 10k lines of duplicated code, tests and docs ([#381](https://github.com/r-near/near-kit-rs/pull/381))
- rewrite the READMEs, crate front pages and MIGRATION.md for 0.30 ([#380](https://github.com/r-near/near-kit-rs/pull/380))
- rewrite the examples as runnable tasks and run them in CI ([#379](https://github.com/r-near/near-kit-rs/pull/379))
- document every public item and build the docs with -D warnings ([#377](https://github.com/r-near/near-kit-rs/pull/377))
- [**breaking**] settle the 0.30 names and error variants ([#371](https://github.com/r-near/near-kit-rs/pull/371))
- [**breaking**] give client-free APIs their own error types ([#365](https://github.com/r-near/near-kit-rs/pull/365))

## [0.20.0](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.20.0-rc.2...near-kit-sandbox-v0.20.0) - 2026-10-27

### Added

- [**breaking**] upgrade to near-account-id 3 and near-global-contracts 0.3 ([#360](https://github.com/r-near/near-kit-rs/pull/360))

### Changed

- The default sandbox image is now the stable nearcore `2.14.0`, replacing `2.14.0-rc.4`.
- The minimum supported Rust version is now 1.95.

## [0.20.0-rc.2] - 2026-10-26

### Fixed

- Refresh the locked Docker transport, TLS and trust-root dependencies to patched
  versions, alongside the near-kit signing dependency security fixes.
- Update the near-kit dependency to 0.20.0-rc.2.

Applications keep independent lockfiles. After selecting this RC, run
`cargo update` and `cargo audit --deny warnings` to refresh and check the
application's resolved dependencies.

## [0.20.0-rc.1](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.19.0...near-kit-sandbox-v0.20.0-rc.1) - 2026-10-14

### Added

- *(sandbox)* default to nearcore 2.14.0-rc.4 and cover 2.14 behavior ([#341](https://github.com/r-near/near-kit-rs/pull/341))

### Changed

- bump default image to `2.14.0-rc.4` (protocol v88 / nearcore 2.14). Use `.version("2.13.4")` to keep the previous default

## [0.19.0](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.18.3...near-kit-sandbox-v0.19.0) - 2026-10-13

### Fixed

- [**breaking**] restore downstream ergonomics for 0.19 ([#327](https://github.com/r-near/near-kit-rs/pull/327))

## [0.18.3](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.18.2...near-kit-sandbox-v0.18.3) - 2026-09-24

### Fixed

- secp256k1 signatures double-hashed, rejected by nearcore ([#324](https://github.com/r-near/near-kit-rs/pull/324))

## [0.18.2](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.18.1...near-kit-sandbox-v0.18.2) - 2026-09-23

### Other

- updated the following local packages: near-kit

## [0.18.1](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.18.0...near-kit-sandbox-v0.18.1) - 2026-09-23

### Other

- record 0.18.0 release notes under their released versions ([#319](https://github.com/r-near/near-kit-rs/pull/319))

## [0.18.0](https://github.com/r-near/near-kit-rs/releases/tag/near-kit-sandbox-v0.18.0) - 2026-09-09

### Added

- Initial publishable companion crate for Docker-backed NEAR sandbox lifecycle,
  client construction, balance patching, and block fast-forwarding.
