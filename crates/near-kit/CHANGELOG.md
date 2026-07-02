# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.12.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.11.2...near-kit-v0.12.0) - 2026-07-02

### Added

- *(types)* impl IntoGlobalContractId for GlobalContractId ([#234](https://github.com/r-near/near-kit-rs/pull/234))
- *(rpc)* thin wrappers for stabilized 2.13 RPC methods ([#232](https://github.com/r-near/near-kit-rs/pull/232))
- *(types)* ExecutionMetadata V4 typed contracts field ([#231](https://github.com/r-near/near-kit-rs/pull/231))
- *(rpc)* view_state with pagination ([#230](https://github.com/r-near/near-kit-rs/pull/230))
- *(keys)* ML-DSA-65 post-quantum signing support (protocol v85) ([#228](https://github.com/r-near/near-kit-rs/pull/228))
- *(sandbox)* default image to 2.13.0-rc.2 (protocol v85) ([#225](https://github.com/r-near/near-kit-rs/pull/225))
- *(types)* Action::DelegateV2 + versioned signed delegate (NEP-611) ([#227](https://github.com/r-near/near-kit-rs/pull/227))
- *(types)* gas-key transacting (TransactionV1 + custom borsh) ([#226](https://github.com/r-near/near-kit-rs/pull/226))

### Added

- *(types)* `impl IntoGlobalContractId for GlobalContractId` so an
  already-constructed `GlobalContractId` can be passed directly to `deploy_from`
  (identity conversion).
- *(types)* gas-key transacting: `TransactionV1` with `TransactionNonce`
  (incl. `GasKeyNonce { nonce, nonce_index }`) and `NonceMode`, the
  backward-compatible custom borsh scheme (V0 tag-less, V1 `0x01`-tagged),
  `VersionedTransaction`/`SignedTransactionV1`, and a gas-key signing path
  (protocol 2.13).
- *(types)* `Action::DelegateV2` (=14) for gas-key meta-transactions (NEP-611):
  `DelegateActionV2` (gas-key-capable `TransactionNonce`),
  `VersionedDelegateActionPayload`, `VersionedSignedDelegateAction`, and the
  distinct V2 NEP-461 signing domain (a V1 delegate signature is not valid for a
  V2 action). `NonDelegateAction` now rejects both delegate variants on the wire.
  Adds the matching RPC views (`ActionView::DelegateV2`,
  `VersionedDelegateActionPayloadView`, `DelegateActionV2View`,
  `TransactionNonceView`) so node-returned `DelegateV2` actions parse.
- *(keys)* ML-DSA-65 post-quantum keys (FIPS 204, protocol v85): `KeyType::MlDsa65`,
  full `PublicKey`/`SecretKey`/`Signature` support (32-byte seed keygen, sign,
  verify), `ml-dsa-65:` string + borsh `[2][1952]` round-trip, and a non-signing
  `ml-dsa-65-hash:` view handle so `view_access_key_list` parses without panicking.
  `SecretKey` accepts both the 32-byte seed and the 4032-byte expanded
  private key that NEAR tooling exports under the `ml-dsa-65:` prefix.
- *(rpc)* `view_state` RPC method with pagination (`after_key_base64` / `limit`
  / `last_key`): `RpcClient::view_state` for a single page and
  `RpcClient::view_state_all` to read a contract's full state across pages.
  Adds `StateItem` / `ViewStateResult` types.
- *(types)* complete the 2.13 typed read path so node-accepted 2.13 transactions
  deserialize through the high-level `FinalExecutionOutcome` / `.send()` path
  instead of forcing raw JSON: `ExecutionMetadata` V4 (`contracts` field +
  `AccountContractView`), `ActionView::TransferToGasKey`/`WithdrawFromGasKey`,
  and `AccessKeyPermissionView::GasKeyFunctionCall`/`GasKeyFullAccess`. JSON wire
  format stays back-compatible (V1–V3 metadata omit `contracts`) (protocol 2.13).
- *(rpc)* thin wrappers for the stabilized 2.13 methods: `RpcClient::block_effects`
  (→ `BlockEffects`), `RpcClient::genesis_config` (raw JSON), and
  `RpcClient::maintenance_windows` (→ `Vec<MaintenanceWindow>`). The renamed
  methods replace the `EXPERIMENTAL_` aliases, which the node still accepts.

### Changed

- *(sandbox)* bump default image to `2.13.0-rc.2` (protocol v85 / nearcore 2.13)

### Fixed

- *(sandbox)* `set_balance` now polls until the patched balance is observable
  instead of relying on a fixed delay, fixing a flaky read-after-write race
  exposed under load on the 2.13 node

## [0.11.2](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.11.1...near-kit-v0.11.2) - 2026-06-23

### Added

- *(types)* add CryptoHash <-> [u8; 32] conversions for near-sdk interop ([#224](https://github.com/r-near/near-kit-rs/pull/224))

### Fixed

- *(sandbox)* remove async from atexit cleanup to avoid TLS panic on exit ([#222](https://github.com/r-near/near-kit-rs/pull/222))
- *(sandbox)* update default image to 2.12.0 ([#221](https://github.com/r-near/near-kit-rs/pull/221))

## [0.11.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.11.0...near-kit-v0.11.1) - 2026-06-17

### Added

- *(macros)* derive Debug and Clone on generated contract clients ([#216](https://github.com/r-near/near-kit-rs/pull/216))

### Fixed

- *(client)* treat empty view/call result as unit instead of failing ([#217](https://github.com/r-near/near-kit-rs/pull/217))

### Other

- lint all features and keep clippy clean on Rust 1.96 ([#218](https://github.com/r-near/near-kit-rs/pull/218))

## [0.11.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.10.0...near-kit-v0.11.0) - 2026-06-04

### Added

- [**breaking**] replace custom NEP-616 types with near-global-contracts 0.2 ([#209](https://github.com/r-near/near-kit-rs/pull/209))

## [0.10.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.9.1...near-kit-v0.10.0) - 2026-06-04

### Added

- add EXPERIMENTAL_receipt_to_tx RPC method ([#207](https://github.com/r-near/near-kit-rs/pull/207))
- add nonce_mode and validator_reward_paid_prev_epoch from nearcore 2.12 ([#206](https://github.com/r-near/near-kit-rs/pull/206))

### Fixed

- handle new nearcore 2.12 error variants and add forward-compat fallbacks ([#205](https://github.com/r-near/near-kit-rs/pull/205))

## [0.9.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.9.0...near-kit-v0.9.1) - 2026-05-26

### Fixed

- add impl FromStr for ChainId ([#201](https://github.com/r-near/near-kit-rs/pull/201))
- decode typed RPC errors from non-2xx responses ([#189](https://github.com/r-near/near-kit-rs/pull/189))

## [0.9.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.8.0...near-kit-v0.9.0) - 2026-04-15

### Added

- interactive-clap feature passthrough + document near-token/near-gas co-dependency ([#187](https://github.com/r-near/near-kit-rs/pull/187))
- enrich tracing spans for CLI teach-me support ([#184](https://github.com/r-near/near-kit-rs/pull/184))
- human-readable Display impls for all error types ([#183](https://github.com/r-near/near-kit-rs/pull/183))
- expose unsigned transaction for external signing workflows ([#182](https://github.com/r-near/near-kit-rs/pull/182))
- add validator and epoch query helpers ([#181](https://github.com/r-near/near-kit-rs/pull/181))
- unified FinalExecutionOutcome and high-level tx_status ([#167](https://github.com/r-near/near-kit-rs/pull/167))

### Fixed

- replace compromised slipped10 with inline SLIP-10 ed25519 ([#186](https://github.com/r-near/near-kit-rs/pull/186))

## [0.8.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.7.2...near-kit-v0.8.0) - 2026-04-10

### Added

- decode execution status ([#172](https://github.com/r-near/near-kit-rs/pull/172))
- type-safe wait levels for transaction submission ([#163](https://github.com/r-near/near-kit-rs/pull/163))

### Fixed

- require chain_id in Near::custom() ([#169](https://github.com/r-near/near-kit-rs/pull/169))

### Other

- use Near::sandbox() in tests and examples ([#173](https://github.com/r-near/near-kit-rs/pull/173))
- implement serde traits for `SecretKey` ([#170](https://github.com/r-near/near-kit-rs/pull/170))
- use serde_with derives for CryptoHash ([#171](https://github.com/r-near/near-kit-rs/pull/171))
- Revert "fix: return SendTxResponse from send_with_options" ([#161](https://github.com/r-near/near-kit-rs/pull/161))

## [0.7.2](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.7.1...near-kit-v0.7.2) - 2026-04-07

### Fixed

- return SendTxResponse from send_with_options ([#159](https://github.com/r-near/near-kit-rs/pull/159))

## [0.7.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.7.0...near-kit-v0.7.1) - 2026-04-06

### Added

- migrate view_function to EXPERIMENTAL_call_function ([#153](https://github.com/r-near/near-kit-rs/pull/153))
- support NEAR_SANDBOX_IMAGE env var and Docker health checks ([#148](https://github.com/r-near/near-kit-rs/pull/148))
- add sandbox_fast_forward RPC method ([#146](https://github.com/r-near/near-kit-rs/pull/146))

### Other

- update near-sandbox to 2.11.0 ([#152](https://github.com/r-near/near-kit-rs/pull/152))
- simplify NonceManager to a single next() method ([#143](https://github.com/r-near/near-kit-rs/pull/143))

## [0.7.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.6.0...near-kit-v0.7.0) - 2026-03-22

### Added

- InMemorySigner implicit account constructors + from_secret_key ergonomics ([#140](https://github.com/r-near/near-kit-rs/pull/140))
- add tracing spans and #[instrument] throughout ([#128](https://github.com/r-near/near-kit-rs/pull/128))
- [**breaking**] rework deploy/publish convenience API ([#130](https://github.com/r-near/near-kit-rs/pull/130))
- enable testcontainers watchdog for container cleanup on Ctrl+C ([#134](https://github.com/r-near/near-kit-rs/pull/134))
- allow overriding max_nonce_retries on Near and per-transaction ([#129](https://github.com/r-near/near-kit-rs/pull/129))
- add semantic helpers to TxExecutionStatus ([#115](https://github.com/r-near/near-kit-rs/pull/115)) ([#125](https://github.com/r-near/near-kit-rs/pull/125))
- [**breaking**] switch sandbox backend from native binary to Docker (testcontainers) ([#111](https://github.com/r-near/near-kit-rs/pull/111))
- derive Ord and PartialOrd for TxExecutionStatus ([#113](https://github.com/r-near/near-kit-rs/pull/113))
- [**breaking**] consolidate transaction error handling ([#103](https://github.com/r-near/near-kit-rs/pull/103))
- [**breaking**] composable typed contract calls via FunctionCall constructors ([#100](https://github.com/r-near/near-kit-rs/pull/100))
- [**breaking**] replace Network enum with ChainId newtype ([#101](https://github.com/r-near/near-kit-rs/pull/101))
- [**breaking**] return FinalExecutionOutcome directly, remove TransactionOutcome newtype ([#95](https://github.com/r-near/near-kit-rs/pull/95))

### Fixed

- eliminate redundant block() RPC call in sign() ([#138](https://github.com/r-near/near-kit-rs/pull/138))
- prevent underflow in Expired retry guard when max_nonce_retries is 0 ([#135](https://github.com/r-near/near-kit-rs/pull/135))
- revert leaked tracing changes and relax testcontainers pin ([#133](https://github.com/r-near/near-kit-rs/pull/133))
- use testcontainers 0.25 for MSRV 1.86 compatibility ([#131](https://github.com/r-near/near-kit-rs/pull/131))
- prevent overflow in nonce retry loop when max_nonce_retries == u32::MAX ([#132](https://github.com/r-near/near-kit-rs/pull/132))
- make InvalidTxError::Expired retryable ([#122](https://github.com/r-near/near-kit-rs/pull/122)) ([#124](https://github.com/r-near/near-kit-rs/pull/124))
- [**breaking**] narrow ExecutionStatus::Failure to ActionError ([#114](https://github.com/r-near/near-kit-rs/pull/114)) ([#126](https://github.com/r-near/near-kit-rs/pull/126))
- [**breaking**] make near.account_id() return &AccountId directly ([#116](https://github.com/r-near/near-kit-rs/pull/116)) ([#127](https://github.com/r-near/near-kit-rs/pull/127))

### Other

- [**breaking**] replace PublicKey/SecretKey/Signature structs with enums ([#141](https://github.com/r-near/near-kit-rs/pull/141))
- use entrypoint env vars for sandbox config ([#139](https://github.com/r-near/near-kit-rs/pull/139))
- remove sandbox bool from Near struct ([#104](https://github.com/r-near/near-kit-rs/pull/104))
- update README examples for v0.6.0 API ([#89](https://github.com/r-near/near-kit-rs/pull/89))

## [0.6.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.5.1...near-kit-v0.6.0) - 2026-03-19

### Added

- [**breaking**] add Near::state_init() and simplify state init API ([#88](https://github.com/r-near/near-kit-rs/pull/88))
- [**breaking**] implement Signer for Arc<T: Signer> instead of Arc<dyn Signer> ([#87](https://github.com/r-near/near-kit-rs/pull/87))
- support NEAR_MAX_NONCE_RETRIES env var and add Debug impls ([#86](https://github.com/r-near/near-kit-rs/pull/86))
- add FunctionCall type for composable transaction building ([#84](https://github.com/r-near/near-kit-rs/pull/84))
- add function_call() one-shot method on TransactionBuilder ([#81](https://github.com/r-near/near-kit-rs/pull/81))
- TransactionOutcome newtype for type-safe transaction results ([#80](https://github.com/r-near/near-kit-rs/pull/80))
- [**breaking**] replace custom types with upstream near-account-id, near-token, near-gas ([#71](https://github.com/r-near/near-kit-rs/pull/71))
- expose signer accessor and fix RotatingSigner public_key ([#70](https://github.com/r-near/near-kit-rs/pull/70))
- add tracing instrumentation to near-kit ([#69](https://github.com/r-near/near-kit-rs/pull/69))
- remove lifetime from typed contract client macro ([#67](https://github.com/r-near/near-kit-rs/pull/67))
- add signer ergonomics (public_key accessor, with_signer on token clients) ([#61](https://github.com/r-near/near-kit-rs/pull/61))

### Fixed

- refactor StorageDepositCall to use CallBuilder ([#79](https://github.com/r-near/near-kit-rs/pull/79))

## [0.5.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.5.0...near-kit-v0.5.1) - 2026-03-16

### Fixed

- match near-sdk serde format for GlobalContractIdentifier ([#59](https://github.com/r-near/near-kit-rs/pull/59))

## [0.5.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.4.3...near-kit-v0.5.0) - 2026-03-16

### Added

- add secp256k1 support for SecretKey, PublicKey, and Signature ([#56](https://github.com/r-near/near-kit-rs/pull/56))
- expose TransactionBuilder::add_action() for flexible action composition ([#55](https://github.com/r-near/near-kit-rs/pull/55))
- add TransactionBuilder::state_init() convenience method ([#49](https://github.com/r-near/near-kit-rs/pull/49))
- add Serialize/Deserialize for Deterministic* and GlobalContract* types ([#50](https://github.com/r-near/near-kit-rs/pull/50))

### Fixed

- address Copilot review feedback on PRs #55 and #56 ([#57](https://github.com/r-near/near-kit-rs/pull/57))

### Other

- add conditional actions and add_action examples to README ([#58](https://github.com/r-near/near-kit-rs/pull/58))
- replace impl AsRef<str> with impl Into<AccountId> for account ID params ([#54](https://github.com/r-near/near-kit-rs/pull/54))
- Make CallBuilder::finish() public for conditional action building ([#48](https://github.com/r-near/near-kit-rs/pull/48))

## [0.4.3](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.4.2...near-kit-v0.4.3) - 2026-03-12

### Added

- configurable max_nonce_retries and network-scoped nonce cache ([#45](https://github.com/r-near/near-kit-rs/pull/45))

## [0.4.2](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.4.1...near-kit-v0.4.2) - 2026-03-04

### Added

- add RotatingSigner::from_signers() and into_inner() on storage signers ([#43](https://github.com/r-near/near-kit-rs/pull/43))

## [0.4.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.4.0...near-kit-v0.4.1) - 2026-03-04

### Added

- add primitives for per-key sequential transaction sends ([#41](https://github.com/r-near/near-kit-rs/pull/41))

## [0.4.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.3.1...near-kit-v0.4.0) - 2026-02-28

### Added

- add validator, light client, and state change types ([#34](https://github.com/r-near/near-kit-rs/pull/34))
- add missing fields and new type variants ([#33](https://github.com/r-near/near-kit-rs/pull/33))
- replace String/Value fields with proper types ([#32](https://github.com/r-near/near-kit-rs/pull/32))
- add gas key types ([#31](https://github.com/r-near/near-kit-rs/pull/31))
- typed error types for transaction execution failures ([#29](https://github.com/r-near/near-kit-rs/pull/29))
- add `Near::with_signer` for multi-account usage ([#28](https://github.com/r-near/near-kit-rs/pull/28))
- update max gas from 300 TGas to 1 PGas ([#23](https://github.com/r-near/near-kit-rs/pull/23))

### Fixed

- replace all serde_json::Value with strict types ([#37](https://github.com/r-near/near-kit-rs/pull/37))
- close critical type parity gaps with nearcore ([#36](https://github.com/r-near/near-kit-rs/pull/36))
- align FinalExecutionStatus and ExecutionStatus with nearcore ([#35](https://github.com/r-near/near-kit-rs/pull/35))

### Other

- add sandbox integration tests for typed error deserialization ([#30](https://github.com/r-near/near-kit-rs/pull/30))
- *(deps)* bump serde_with from 3.16.1 to 3.17.0 ([#26](https://github.com/r-near/near-kit-rs/pull/26))

## [0.3.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.3.0...near-kit-v0.3.1) - 2026-02-03

### Other

- update to Rust 2024 edition and resolver v3 ([#15](https://github.com/r-near/near-kit-rs/pull/15))

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
