# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.15.0-rc.3](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.15.0-rc.2...near-kit-macros-v0.15.0-rc.3) - 2026-10-30

### Changed without a compile error

- A method's parameters are sent by name, as near-sdk's `#[near]` and `#[ext_contract]` encode them, including a single parameter: `fn m(&self, args: Args)` used to send `Args` as the whole argument object and now sends `{"args": ..}`. Borsh bytes are unchanged, and methods without parameters still send `{}` (JSON) or nothing (Borsh). See [MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#one-parameter-is-sent-by-name) ([#393](https://github.com/r-near/near-kit-rs/pull/393)).

### Added

- `#[args]` on a method's only parameter sends it as the whole argument object, as a lone parameter was sent before ([#393](https://github.com/r-near/near-kit-rs/pull/393)).
- Methods may take several parameters: a JSON object with a key per parameter in order (`None` as `null`, raw identifiers without `r#`), or each parameter's Borsh encoding in order. Borrowed and `impl Trait` parameters work, and a type that can't be encoded is reported at its parameter ([#393](https://github.com/r-near/near-kit-rs/pull/393)).
- `#[args]` next to other parameters, on the method or with options, other parameter attributes (such as `#[serde(..)]`) and repeated parameter names are compile errors ([#393](https://github.com/r-near/near-kit-rs/pull/393)).

## [0.15.0-rc.2](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.15.0-rc.1...near-kit-macros-v0.15.0-rc.2) - 2026-10-29

- `#[call(gas = EXPR, deposit = EXPR)]` sets a call's default gas and deposit. Each value is a `Gas` or `NearToken` expression, evaluated each time the call is built. The client method and the `FunctionCall` constructor both start from it, and `.gas(..)`/`.deposit(..)` still override it. `gas` or `deposit` on a view, an unknown or repeated key, and a second `#[call]` are compile errors ([#389](https://github.com/r-near/near-kit-rs/pull/389)).
- No-argument methods still send `{}` (JSON) or no bytes (Borsh), now matching `View::json` and `FunctionCall::new` in near-kit-primitives 0.30.0-rc.2 ([#388](https://github.com/r-near/near-kit-rs/pull/388)).

## [0.15.0-rc.1](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.14.0...near-kit-macros-v0.15.0-rc.1) - 2026-10-29

Use this crate through `near-kit` or `near-kit-primitives` with the `contracts`
feature, not directly.

- [**breaking**] Generates `<Name>Client<C>`, generic over any `Viewer`/`TxSender` backend (`C = Near` with near-kit's `rpc`) ([#368](https://github.com/r-near/near-kit-rs/pull/368)).
- [**breaking**] Rejects forms it used to mis-generate: ignored `#[call]` return types, pattern arguments, generic, lifetime or `async` methods, and the reserved names `contract_id`, `client` and `clone`. `#[cfg]` on a method now applies to the generated items ([#375](https://github.com/r-near/near-kit-rs/pull/375)).
- [**breaking**] Raw method names are sent without `r#` (`fn r#type` sends `type`) ([#375](https://github.com/r-near/near-kit-rs/pull/375)).
- [**breaking**] Removed the standalone `call`/`json`/`borsh` markers.
- Works with `near-kit-primitives` alone ([#367](https://github.com/r-near/near-kit-rs/pull/367)).

## [0.14.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.13.1...near-kit-macros-v0.14.0) - 2026-10-13

### Fixed

- [**breaking**] restore downstream ergonomics for 0.19 ([#327](https://github.com/r-near/near-kit-rs/pull/327))

## [0.13.1](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.13.0...near-kit-macros-v0.13.1) - 2026-09-23

### Other

- record 0.18.0 release notes under their released versions ([#319](https://github.com/r-near/near-kit-rs/pull/319))

## [0.13.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.12.1...near-kit-macros-v0.13.0) - 2026-09-09

### Changed

- **Breaking:** reject options such as `#[call(payable)]`; `#[call]` is now only a
  marker for change methods, and attached deposits remain explicit call-builder
  configuration.
- **Breaking:** require `#[json]` and `#[borsh]` to be option-free, unique, and
  mutually exclusive on each contract method.
- Resolve the downstream `near-kit` crate name during macro expansion, so generated
  clients work when the dependency is renamed in `Cargo.toml`.
- Generate contract clients against near-kit's curated `rpc`, `transaction`, and
  `signer` facades instead of removed root-level re-exports.

## [0.12.1](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.12.0...near-kit-macros-v0.12.1) - 2026-08-11

### Other

- *(deps)* bump syn from 2.0.119 to 3.0.3 ([#265](https://github.com/r-near/near-kit-rs/pull/265))

## [0.12.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.12.0-rc.1...near-kit-macros-v0.12.0) - 2026-07-13

First stable release of the 0.12 line. No changes since `0.12.0-rc.1`.

## [0.11.1](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.11.0...near-kit-macros-v0.11.1) - 2026-06-17

### Added

- *(macros)* derive Debug and Clone on generated contract clients ([#216](https://github.com/r-near/near-kit-rs/pull/216))

## [0.10.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.9.1...near-kit-macros-v0.10.0) - 2026-06-04

### Other

- update Cargo.toml dependencies

## [0.7.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.6.0...near-kit-macros-v0.7.0) - 2026-03-22

### Added

- [**breaking**] composable typed contract calls via FunctionCall constructors ([#100](https://github.com/r-near/near-kit-rs/pull/100))

## [0.6.0](https://github.com/r-near/near-kit-rs/compare/near-kit-macros-v0.5.1...near-kit-macros-v0.6.0) - 2026-03-19

### Added

- remove lifetime from typed contract client macro ([#67](https://github.com/r-near/near-kit-rs/pull/67))

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
