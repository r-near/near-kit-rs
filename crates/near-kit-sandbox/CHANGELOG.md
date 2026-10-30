# Changelog

All notable changes to `near-kit-sandbox` are documented in this file.

## [Unreleased]

## [0.30.0-rc.3](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.30.0-rc.2...near-kit-sandbox-v0.30.0-rc.3) - 2026-10-30

### Added

- switch accounts and pick a wait level per transaction without losing the sender's settings; accept boxed backends ([#394](https://github.com/r-near/near-kit-rs/pull/394))
- *(macros)* [**breaking**] send #[contract] parameters by name, like near-sdk, so traits can mirror a contract's signatures ([#393](https://github.com/r-near/near-kit-rs/pull/393))
- derive 0s account ids without a deposit and take either client via AsRef<Near> ([#391](https://github.com/r-near/near-kit-rs/pull/391))

## [0.30.0-rc.2](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.30.0-rc.1...near-kit-sandbox-v0.30.0-rc.2) - 2026-10-29

Depends on near-kit 0.30.0-rc.2. No user-facing changes; version bumped with
the workspace.

## [0.30.0-rc.1](https://github.com/r-near/near-kit-rs/compare/near-kit-sandbox-v0.20.1...near-kit-sandbox-v0.30.0-rc.1) - 2026-10-29

Depends on near-kit 0.30.0-rc.1. See
[MIGRATION.md](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#sandbox-near-kit-sandbox).

- [**breaking**] Configure with `Sandbox::builder()` (`.shared()` / `.fresh()`), which replaces `SandboxConfig`.
- [**breaking**] `sandbox.root_account()` returns a `SigningNear`, replacing `client()`, `rpc_url()`, `root_account_id()`, `chain_id()` and `root_signer()`.
- [**breaking**] Account arguments are typed (`impl Into<AccountId>`), and `SANDBOX_ROOT_ACCOUNT` is a `&AccountIdRef`. Removed `SandboxError::InvalidRootAccount`, `InvalidAccount` and `RootSigner`, and `SandboxError` is `#[non_exhaustive]` ([#381](https://github.com/r-near/near-kit-rs/pull/381), [#383](https://github.com/r-near/near-kit-rs/pull/383)).
- `sandbox.patch_state(records)` replaces the `RpcClient` method and no longer sleeps 100 ms afterwards. Poll for the state, or use `set_balance`, which does.
- The free functions `patch_state(&rpc, records)` and `fast_forward(&rpc, n)` work against any sandbox node, including one you started yourself. A `Sandbox` builds one `Near` and shares it ([#383](https://github.com/r-near/near-kit-rs/pull/383)).
- Starting a sandbox returns `SandboxError::Container` instead of panicking when its HTTP client can't be built, and builds it before the container starts ([#385](https://github.com/r-near/near-kit-rs/pull/385)).

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
