# Changelog

All notable changes to `near-kit-sandbox` are documented in this file.

## [Unreleased]

## [0.20.0](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.19.0...near-kit-sandbox-v0.20.0) - 2026-10-14

### Added

- *(sandbox)* default to nearcore 2.14.0-rc.4 and cover 2.14 behavior ([#341](https://github.com/r-near/near-kit-rs/pull/341))

### Changed

- bump default image to `2.14.0-rc.4` (protocol v88 / nearcore 2.14)

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
