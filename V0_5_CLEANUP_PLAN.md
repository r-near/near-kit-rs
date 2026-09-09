# near-kit v0.5 cleanup plan

This document is the durable execution plan for `chore/v0.5-deslopification`.
The `v0.5` name is the cleanup-program label; the eventual published semver is
a separate release decision.

## Objective

Make near-kit smaller and easier to use without removing the capabilities that
justify the crate. Correctness comes before API contraction, and API contraction
comes before dependency reshuffling.

## Invariants

- Keep the high-level `Near` and low-level `RpcClient` layers.
- Keep typed `WaitLevel` response behavior; consolidate its implementation only.
- Keep actionable RPC/NEP-641 distinctions, but move raw detail out of the root.
- Keep protocol wire support for every key/signature variant.
- Keep one-shot `view`, `call`, and `transfer` conveniences and a generic RPC
  escape hatch.
- Do not silently replace invalid input or failed serialization with defaults.
- Do not panic on ordinary runtime input accepted by a fallible public API.
- Preserve Rust 1.88, native, WASM, and WASI compatibility unless a deliberate
  change is documented before it lands.
- Prefer deleting concepts to moving the same complexity into more crates.

## Baseline

- Starting point: clean `main` at `91bb2b4` (`near-kit-v0.17.0`).
- Library source: about 20.7k production lines plus 11.4k inline-test lines.
- Integration tests: about 9.7k lines.
- Default build: about 246 normal transitive packages; all features about 350.
- Baseline verification: 586 unit tests pass; Rust 1.88 all-features/all-targets
  checking passes.

## Phase 0: correctness gates

- Propagate JSON/Borsh argument serialization failures.
- Keep transaction retries on one claimed signing key and pair nonce state with
  that key.
- Make expired-transaction retries honor `max_nonce_retries` exactly.
- Replace builder parsing panics with typed input or deferred builder errors.
- Remove unsafe `FtAmount` token arithmetic and use checked decimal scaling.
- Reject deploy `ActionView` conversions that contain only code hashes.
- Add focused regressions for each defect.

Phase 0 must be green before breaking API work begins.

## Phase 1: no-regret deletion and consolidation

- Remove source-dead errors, stale docs, unused convenience methods, and no-op
  macro options.
- Make one function-call payload canonical; stop `CallBuilder` from forwarding
  the complete transaction API.
- Share transaction preparation/signing stages across build, sign, offline sign,
  delegate, and send paths.
- Share query block options and response decoding internally.
- Make FT/NFT clients compose `Near` rather than copying its state.
- Consolidate request-independent RPC error parsing and one context-enrichment
  step without losing public error semantics.
- Remove production dependencies used only by examples and unused dev
  dependencies.

## Phase 2: v0.5 public surface

- Replace root glob exports with a small curated prelude.
- Put advanced APIs under explicit `rpc`, `transaction`, `signer`, `protocol`,
  and `standards` namespaces.
- Keep short migration aliases only when they materially reduce adoption cost.
- Unify exact duplicate concepts such as nonce and global-deploy modes.
- Decide the honest signer extension seam: public custom async backend or a
  smaller locally implementable contract.
- Document the before/after API and breaking changes.

## Phase 3: dependency and ownership boundaries

- Make the default feature set `rpc` unless measurements justify one addition.
- Make keyring, file signing, WASI transport, typed-contract macros, mnemonic/HD
  operations, and specialized local crypto explicitly opt-in.
- Move Docker/testcontainers sandbox lifecycle out of the core client while
  retaining a simple network/client adapter.
- Move mutable known-token address data out of core.
- Keep FT/NFT helpers only if they remain a small layer over `Near`; otherwise
  move them to an extension boundary.
- Keep a deliberately small private wire subset plus upstream conformance tests;
  do not adopt the full nearcore dependency graph without an MSRV/WASM/size
  experiment.

## Phase 4: test and release hygiene

- Replace duplicated sandbox/account fixtures with one support module.
- Replace print-oriented debug suites with assertions or manual diagnostics.
- Keep end-to-end tests for owned behavior; stop retesting upstream unit/account
  implementations.
- Mark feature-dependent examples with `required-features`.
- Align README, crate docs, feature tables, versions, and package contents.
- Verify formatting, focused regressions, workspace tests, clippy, MSRV,
  no-default/default/all features, WASM/WASI targets, examples, and packaging.

## Review and commit strategy

Use small Conventional Commit units in this order:

1. `fix:` correctness defects and regressions.
2. `refactor:` private consolidation with behavior held constant.
3. `feat!:` or `refactor!:` intentional public API contraction.
4. `chore:` feature/dependency/test/package cleanup.
5. `docs:` migration guide and final surface documentation.

Do not mix unrelated public breaks into correctness commits. After every phase,
review `cargo public-api`-equivalent output or a generated rustdoc surface diff
before continuing.

## Completion criteria

- Every confirmed correctness defect has a regression test.
- The root namespace is curated and documented.
- There is one canonical call payload and one transaction-preparation flow.
- Defaults exclude credential stores, Docker, and environment-specific adapters.
- Sandbox lifecycle and mutable token catalogs are not core-client obligations.
- Supported targets and MSRV pass the full matrix.
- The branch contains a migration guide and a coherent, reviewable commit series.

## Completion snapshot

The cleanup program ships as the 0.18 line: `near-kit-macros` 0.13.0,
`near-kit` 0.18.0, and the new `near-kit-sandbox` 0.18.0 companion crate.

- Main-crate production Rust fell from 20,744 to 19,115 physical lines
  (-7.9%); total test Rust fell from 21,331 to 16,943 (-20.6%).
- Sandbox integration coverage fell from 9,562 lines / 199 tests to 4,573
  lines / 85 behavior-focused tests, with Docker lifecycle owned by the
  companion crate.
- near-kit's normal dependency graph fell from 246 to 219 packages with
  defaults and from 351 to 236 with all features; the default feature set is
  now exactly `rpc`.
- The final native workspace, strict Clippy/rustdoc, Rust 1.88, browser Wasm,
  WASI, macro trybuild, and sandbox compile-only matrices passed without
  starting Docker.
- Publish in dependency order: `near-kit-macros` first, then `near-kit`, then
  `near-kit-sandbox`, rerunning package verification after each release reaches
  the registry index.

## Review refresh — 2026-09-09

- Upstream remains at 0.17.0; the registry still has macros 0.12.1 and core
  0.17.0, so the planned release versions remain available.
- Independent read-only API/correctness and documentation/package reviews found
  no functional blockers. Added the core changelog and fixed the packaged README
  link to the sandbox guide.
- Docker execution exposed a finality race in the pre-signed transfer test;
  wait for `Final` before querying the final balance. Ordinary and protocol
  fixtures now pin stable sandbox 2.13.4 instead of release-candidate/moving tags.
- Refreshed native tests, strict Clippy/rustdoc, Rust 1.88, browser/WASI checks,
  macro compile tests, and default/offline doctests. Docker execution is now
  part of the review verification, not only a compile check.
- The macros package verifies locally. Core packaging awaits published macros
  0.13.0, and sandbox packaging awaits published core 0.18.0; rerun each package
  verification in dependency order during separately authorized release work.
