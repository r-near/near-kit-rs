# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.30.0-rc.2](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.30.0-rc.1...near-kit-v0.30.0-rc.2) - 2026-10-29

### Added

- *(macros)* declare a call's default gas and deposit in #[contract] ([#389](https://github.com/r-near/near-kit-rs/pull/389))

### Fixed

- send `{}` from every JSON view and call without arguments ([#388](https://github.com/r-near/near-kit-rs/pull/388))

### Other

- use US English spelling throughout ([#386](https://github.com/r-near/near-kit-rs/pull/386))

## [0.30.0-rc.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.20.1...near-kit-v0.30.0-rc.1) - 2026-10-29

This release changes the whole API in one go and keeps no deprecated aliases.
[MIGRATION.md](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md)
maps every removed method to its replacement. The new `near_kit::guide`
module explains the model.

### Changed without a compile error

Read these first. Code that compiles against this release can still behave
differently:

- **Sends wait for `Final` by default** (0.20.1 used `ExecutedOptimistic`).
  This also applies to `near.send_signed(..)`. Use `.wait_until::<ExecutedOptimistic>()`
  to keep the old behavior.
  ([MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#wait-levels-final-by-default))
- **Cached nonces are shared per chain.** They are keyed by genesis hash,
  account and key rather than by RPC URL, so every client asks its node for
  `status` before its first nonce. A test transport that panics on unexpected
  methods needs a `status` reply. A backend without `status` keys nonces by
  chain ID instead.
  ([MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#nonces))
- **Ed25519 secret keys print in the 64-byte form** that nearcore, near-cli-rs
  and neard key files read. Both forms still parse, and 64-byte keys whose
  halves disagree are now rejected.
  ([MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#keys-and-signers-1))
- **RPC errors are retried by cause and status.** A `-32000` error is retried
  only when it has no cause or a transient one. Unknown transactions,
  untracked shards and receipt lookups now have typed, non-retryable errors.
  A JSON-RPC error on a 4xx keeps its status's classification, so 408 and 429
  are retried, and `-32601` (method not found) stays `RpcError::Rpc` at any
  status. ([#382](https://github.com/r-near/near-kit-rs/pull/382),
  [MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#rpc-errors-and-retries))
- **An outcome that fails to decode is now an error.** It used to read as
  `outcome: None`. `ExecutionStatus::InvalidTx` is new.
  ([MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#transaction-outcomes))
- **Some error messages read differently.** A local check prints
  "Refused before signing: …" (`Error::Validation`), and an action failure
  prints "Action #N: …" from both `failure_message()` and `result()`, as
  nearcore does. Code that matches on error text needs the new text.
  ([#383](https://github.com/r-near/near-kit-rs/pull/383),
  [MIGRATION](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md#error-variants))
- **`Debug` output and `tracing` spans show only the RPC URL's scheme and
  host**, never its path, query string or credentials, where providers put API
  keys. `RpcClient::url()` still returns the full URL.
  ([#382](https://github.com/r-near/near-kit-rs/pull/382), [#385](https://github.com/r-near/near-kit-rs/pull/385))

### Changed (breaking)

- **Reading and signing are separate clients.** `Near` only reads.
  `near.with_signer(signer)` returns a `SigningNear<W>`, whose type carries
  the wait level. `SigningNear::from_env()` replaces `Near::from_env()`.
  ([#368](https://github.com/r-near/near-kit-rs/pull/368))
- **Everything runs on two traits, `Viewer` and `TxSender`.** Typed contract
  clients, `View`, `Call` and the FT/NFT clients work over any backend that
  implements them, including your own.
  ([#368](https://github.com/r-near/near-kit-rs/pull/368))
- **A transaction is a receiver plus `Action` values**:
  `alice.transaction(id).add_action(..).await`. The per-action builder methods
  are gone. A state init or delegate sent to the wrong receiver now fails with
  `Error::Validation` before signing.
- **`Near` keeps only the reads that do more than forward one RPC call**
  (`view`, `balance`, `account_exists`, `tx_status`). Everything else is
  `near.rpc().<method>(..)`. Reads pick a block with `.at(..)`.
- **Arguments are typed** (`AccountId`/`&AccountIdRef`, `NearToken`, `Gas`).
  `account!("..")` checks a literal at compile time. FT `transfer` and
  `transfer_call` take `amount: u128`, so a literal amount compiles.
- **Each layer has its own error type**: `ContractError<E>`, `OutcomeError`,
  `CodecError`, `SignerError`, `DecodeTransactionError`, `DecodeDelegateError`.
  Each converts into `near_kit::Error` with `?`. `RpcError::Rpc` gained a
  `cause` field. The `RpcError::is_*` helpers were removed.
  ([#365](https://github.com/r-near/near-kit-rs/pull/365), [#371](https://github.com/r-near/near-kit-rs/pull/371), [#381](https://github.com/r-near/near-kit-rs/pull/381))
- **Types that may grow are `#[non_exhaustive]`**: `Error`, `KeyStoreError`,
  `DecodeDelegateError`, `ViewFunctionResult` and `TransportResponse`. A
  `match` needs a `_` arm; build a `TransportResponse` with
  `TransportResponse::new(status, body)`. Base64 errors carry a `String`
  instead of `base64::DecodeError`.
  ([#383](https://github.com/r-near/near-kit-rs/pull/383))
- **`FinalExecutionOutcome` and the other outcomes are `#[must_use]`.**
  `sign_delegate` replaces the builder's `delegate`, and there are new helpers
  for relayers. ([#372](https://github.com/r-near/near-kit-rs/pull/372))
- **The protocol types moved into `near-kit-primitives`.** They are
  re-exported at the same `near_kit::` paths but are new types to semver
  tooling. Several constructors and helpers were removed in favor of plain
  struct and enum syntax (see the protocol types table in MIGRATION).
  ([#366](https://github.com/r-near/near-kit-rs/pull/366), [#381](https://github.com/r-near/near-kit-rs/pull/381))
- **The key loader types are now functions** (`from_credentials`,
  `from_credentials_file`, `from_keyring`, `from_env`).
- **`#[near_kit::contract]` generates `<Name>Client<C = Near>`** and rejects
  forms it used to compile into something wrong (ignored `#[call]` return
  types, pattern arguments, generic or `async` methods, reserved names).
  ([#375](https://github.com/r-near/near-kit-rs/pull/375))
- Removed surface with no users: the `Box<T>` forwarding impls of
  `Viewer`/`TxSender`, `From<NearBuilder> for Near` (call `build()`),
  `NearBuilder::chain_id`, `RpcClient::block_effects`/`maintenance_windows`
  (reachable with `RpcClient::request`), `Default for ChainId` and internal
  constants. ([#381](https://github.com/r-near/near-kit-rs/pull/381), [#383](https://github.com/r-near/near-kit-rs/pull/383))

### Added

- More key forms load: near-api-js's 96-byte secp256k1 form, the 4032-byte
  expanded ML-DSA-65 key, and near-cli-rs `ml-dsa-65-hash:` keyring entries.
  NEP-413 payloads signed with any key type now read back.
  ([#374](https://github.com/r-near/near-kit-rs/pull/374))
- `from_credentials` also finds the per-key files near-cli-rs writes under
  `<network>/<account>/`, and lists them when there are several.
  ([#383](https://github.com/r-near/near-kit-rs/pull/383))
- `max_nonce_retries` also covers `NonceTooLarge` and `Expired`.
  ([#373](https://github.com/r-near/near-kit-rs/pull/373))
- `contracts` without `rpc` now also generates the backend-generic client.
  ([#367](https://github.com/r-near/near-kit-rs/pull/367))
- `MaybeSend`/`MaybeSync` bounds, so code that forwards to `Viewer`/`TxSender`
  also builds on `wasm32-unknown-unknown`.
- A `guide` module with task-oriented chapters, every public item documented,
  and runnable examples checked in CI.
  ([#377](https://github.com/r-near/near-kit-rs/pull/377), [#378](https://github.com/r-near/near-kit-rs/pull/378), [#379](https://github.com/r-near/near-kit-rs/pull/379), [#380](https://github.com/r-near/near-kit-rs/pull/380))

### Fixed

- RPC outcomes decode the way nearcore sends them.
  ([#376](https://github.com/r-near/near-kit-rs/pull/376))
- A universal account bootstrap is signed with `bootstrap_nonce + 1`, read
  from the account view, on every attempt.
  ([#373](https://github.com/r-near/near-kit-rs/pull/373))
- `DelegateOptions { block_height_offset: Some(u64::MAX), .. }` no longer
  overflows into an already-expired delegate action.
  ([#382](https://github.com/r-near/near-kit-rs/pull/382))
- The WASI transport reports a refused connection as the host's error (for
  example `ConnectionRefused`) instead of "request body write failed", and
  aborts the request body before waiting for that error.
  ([#382](https://github.com/r-near/near-kit-rs/pull/382), [#385](https://github.com/r-near/near-kit-rs/pull/385))

The minimum supported Rust version is still 1.95. Each crate's public API is
now checked in CI against a committed snapshot
([#370](https://github.com/r-near/near-kit-rs/pull/370)).

## [0.20.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.20.0...near-kit-v0.20.1) - 2026-10-27

### Other

- update Cargo.toml dependencies

## [0.20.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.20.0-rc.2...near-kit-v0.20.0) - 2026-10-27

### Added

- [**breaking**] upgrade to near-account-id 3 and near-global-contracts 0.3 ([#360](https://github.com/r-near/near-kit-rs/pull/360))

### Changed

- The minimum supported Rust version is now 1.95, as near-global-contracts 0.3 requires.

## [0.20.0-rc.2] - 2026-10-26

### Fixed

- Refresh the locked TLS, trust-root and signing dependencies, including the
  distrusted emSign certificate and constant-time arithmetic security fixes.
- Clear the workspace's known vulnerability, unsoundness and yanked-package
  audit findings. Enable Dependabot security updates and daily dependency audits.

### Security update for applications

Applications keep independent lockfiles. After selecting this RC, run
`cargo update` and `cargo audit --deny warnings`; updating near-kit alone can
retain vulnerable transitive versions. Native RPC requires rustls >=0.23.46 and
rustls-webpki >=0.103.16. Signing requires crypto-bigint >=0.7.6,
ctutils >=0.4.4 and cmov >=0.5.4. Browser and WASI TLS is provided by the host;
the signing dependency updates also apply to offline and wasm builds.

## [0.20.0-rc.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.19.0...near-kit-v0.20.0-rc.1) - 2026-10-14

### Added

- typed construction of nearcore 2.14 universal accounts ([#339](https://github.com/r-near/near-kit-rs/pull/339))
- [**breaking**] decode nearcore 2.14 TIMEOUT_ERROR cause ([#338](https://github.com/r-near/near-kit-rs/pull/338))
- [**breaking**] expose universal account state on AccountView ([#336](https://github.com/r-near/near-kit-rs/pull/336))
- [**breaking**] decode the nearcore 2.14 UniversalStateInit action ([#335](https://github.com/r-near/near-kit-rs/pull/335))
- [**breaking**] decode nearcore 2.14 error variants ([#333](https://github.com/r-near/near-kit-rs/pull/333))

## [0.19.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.18.3...near-kit-v0.19.0) - 2026-10-13

### Fixed

- [**breaking**] restore downstream ergonomics for 0.19 ([#327](https://github.com/r-near/near-kit-rs/pull/327))

## [0.18.3](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.18.2...near-kit-v0.18.3) - 2026-09-24

### Fixed

- secp256k1 signatures double-hashed, rejected by nearcore ([#324](https://github.com/r-near/near-kit-rs/pull/324))

## [0.18.2](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.18.1...near-kit-v0.18.2) - 2026-09-23

### Added

- add CallBuilder::into_call and FunctionCall -> FunctionCallAction conversion ([#322](https://github.com/r-near/near-kit-rs/pull/322))

## [0.18.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.18.0...near-kit-v0.18.1) - 2026-09-23

### Added

- expose a transaction builder's receiver and actions via into_parts ([#320](https://github.com/r-near/near-kit-rs/pull/320))
- make FunctionCall available without RPC ([#316](https://github.com/r-near/near-kit-rs/pull/316))
- preserve metadata on typed view results ([#315](https://github.com/r-near/near-kit-rs/pull/315))

### Other

- record 0.18.0 release notes under their released versions ([#319](https://github.com/r-near/near-kit-rs/pull/319))

## [0.18.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.17.0...near-kit-v0.18.0) - 2026-09-09

### Changed

- **Breaking:** curate the public API into `rpc`, `transaction`, `signer`,
  `protocol`, and `standards` namespaces; remove redundant builder shortcuts.
- **Breaking:** default to `rpc` only; make contract macros and mnemonic support
  opt-in, and move Docker support to `near-kit-sandbox`.
- **Breaking:** make keys opaque, accept only seeds for ML-DSA secret-key import,
  and remove token catalogs and metadata-only amount arithmetic.
- See the [0.18 migration guide](https://github.com/r-near/near-kit-rs/blob/main/MIGRATION.md)
  for import, feature, builder, and signer changes.

### Fixed

- Return caller-input and serialization errors instead of panicking or silently
  substituting values in builders; reject excess precision and overflow.
- Keep one claimed signer key across retries and bound expired-transaction retries.
- Reject conversion of hash-only RPC deploy views into executable deploy actions.

## [0.17.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.16.0...near-kit-v0.17.0) - 2026-08-19

### Added

- [**breaking**] return block_height/block_hash from view_state_all via ViewStateAllResult ([#303](https://github.com/r-near/near-kit-rs/pull/303))
- [**breaking**] paginate view_access_key_list (after_key/limit/last_key) and map TOO_MANY_ACCESS_KEYS ([#305](https://github.com/r-near/near-kit-rs/pull/305))

### Fixed

- map GARBAGE_COLLECTED_BLOCK to UnknownBlock; scope tracing test subscriber ([#309](https://github.com/r-near/near-kit-rs/pull/309))
- [**breaking**] carry block context on ContractStateTooLarge (and other lossy handler errors); map UNKNOWN_GAS_KEY ([#304](https://github.com/r-near/near-kit-rs/pull/304))

### Other

- de-flake global contract sandbox tests by waiting for deploy finality ([#306](https://github.com/r-near/near-kit-rs/pull/306))

## [0.16.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.15.0...near-kit-v0.16.0) - 2026-08-19

### Added

- add TryFrom<ActionView> for Action and document that Deploy* views carry the code hash ([#290](https://github.com/r-near/near-kit-rs/pull/290))
- [**breaking**] include block_height/block_hash on ContractCodeView and ViewStateResult ([#291](https://github.com/r-near/near-kit-rs/pull/291))
- *(keys)* export ML-DSA-65 secret keys in nearcore's 4032-byte expanded form ([#287](https://github.com/r-near/near-kit-rs/pull/287))
- *(keys)* add ML-DSA-65 seed-phrase generation defaulting to 24 words (NEP-649) ([#278](https://github.com/r-near/near-kit-rs/pull/278))

### Fixed

- always paginate view_state_all so page_size 0 doesn't trip TOO_LARGE_CONTRACT_STATE ([#298](https://github.com/r-near/near-kit-rs/pull/298))
- [**breaking**] model ML-DSA-65 view handles as PublicKeyHandle instead of a PublicKey variant ([#293](https://github.com/r-near/near-kit-rs/pull/293))
- read contract_account_id for TOO_LARGE_CONTRACT_STATE ([#297](https://github.com/r-near/near-kit-rs/pull/297))
- stop RpcClient::call retrying InvalidNonce; add RetryConfig::none() ([#289](https://github.com/r-near/near-kit-rs/pull/289))
- *(tracing)* log returned RPC/transaction errors at DEBUG, not WARN/ERROR ([#292](https://github.com/r-near/near-kit-rs/pull/292))
- classify legacy string-form vm errors into typed view-call variants ([#288](https://github.com/r-near/near-kit-rs/pull/288))

### Other

- drop hand-written changelog entry (release-plz owns the changelog) ([#296](https://github.com/r-near/near-kit-rs/pull/296))

## [0.15.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.14.0...near-kit-v0.15.0) - 2026-08-11

### Added

- derive Copy for BlockReference ([#274](https://github.com/r-near/near-kit-rs/pull/274))
- derive ML-DSA-65 keys from BIP-39 seed phrases ([#261](https://github.com/r-near/near-kit-rs/pull/261))

### Fixed

- [**breaking**] distinguish view function RPC errors ([#272](https://github.com/r-near/near-kit-rs/pull/272))

### Other

- *(deps)* unify rand_core on 0.10 by bumping k256 and rand ([#268](https://github.com/r-near/near-kit-rs/pull/268))
- *(deps)* bump ed25519-dalek from 2.2.0 to 3.0.0 ([#241](https://github.com/r-near/near-kit-rs/pull/241))
- *(deps)* bump gloo-timers from 0.3.0 to 0.4.0 ([#264](https://github.com/r-near/near-kit-rs/pull/264))

### Changed

- [**breaking**] preserve block height and hash metadata on applicable view RPC
  error variants when returned by the node

### Fixed

- distinguish missing accounts, undeployed contracts, missing methods, and
  contract panics returned by view function calls

### Removed

- [**breaking**] remove the unused `RpcError::FunctionCall` variant and
  `RpcError::function_call` constructor

## [0.14.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.13.0...near-kit-v0.14.0) - 2026-07-24

### Added

- pluggable RPC transport with wasm32-wasip2 (wasi:http) support ([#262](https://github.com/r-near/near-kit-rs/pull/262))
- make the RPC client optional via a default-on `rpc` feature ([#258](https://github.com/r-near/near-kit-rs/pull/258))

## [0.13.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.12.1...near-kit-v0.13.0) - 2026-07-22

### Added

- put tracing behind a feature flag ([#256](https://github.com/r-near/near-kit-rs/pull/256))
- add query builders for contract code and global contract code ([#255](https://github.com/r-near/near-kit-rs/pull/255))
- [**breaking**] make transaction wait levels type-only ([#252](https://github.com/r-near/near-kit-rs/pull/252))

## [0.12.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.12.0...near-kit-v0.12.1) - 2026-07-17

### Added

- add near-cli migration prerequisites ([#247](https://github.com/r-near/near-kit-rs/pull/247))

### Other

- inject custom HTTP clients ([#249](https://github.com/r-near/near-kit-rs/pull/249))

## [0.12.0](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.12.0-rc.3...near-kit-v0.12.0) - 2026-07-13

### Added

- add wasm32 support ([#150](https://github.com/r-near/near-kit-rs/pull/150))

## [0.12.0-rc.3](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.12.0-rc.2...near-kit-v0.12.0-rc.3) - 2026-07-13

### Added

- *(client)* surface partial tx outcome at early wait levels ([#243](https://github.com/r-near/near-kit-rs/pull/243))

### Fixed

- *(sandbox)* reject mainnet/testnet chain_id with a clear error ([#238](https://github.com/r-near/near-kit-rs/pull/238))

## [0.12.0-rc.2](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.12.0-rc.1...near-kit-v0.12.0-rc.2) - 2026-07-07

### Added

- *(nep413)* support custom nonce schemes in verification ([#237](https://github.com/r-near/near-kit-rs/pull/237))

## [0.12.0-rc.1](https://github.com/r-near/near-kit-rs/compare/near-kit-v0.11.2...near-kit-v0.12.0-rc.1) - 2026-07-02

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
