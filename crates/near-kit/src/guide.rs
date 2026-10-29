//! Task-oriented guides to near-kit, one chapter per topic.
//!
//! | Chapter | Covers |
//! |---------|--------|
//! | [`clients`] | `Near` and `SigningNear`, networks and RPC endpoints, client settings |
//! | [`keys`] | Key types and string forms, loading keys, seed phrases, external signers |
//! | [`transactions`] | Building transactions from actions, batching calls, global contracts, state init |
//! | [`finality`] | Read finality, wait levels and their responses, reading your own writes |
//! | [`nonces`] | The nonce cache, nonce retries, concurrent senders, rotating keys |
//! | [`errors`] | Failed actions vs errors, error types, retries, your own error enum |
//! | [`meta_transactions`] | Signing delegate actions and relaying them |
//! | [`offline_signing`] | Signing without sending, fully offline transactions, NEP-413 messages |
//! | [`contracts_and_tokens`] | `#[contract]` clients, FT and NFT helpers |
//! | [`testing`] | Mock backends, canned RPC responses, the Docker sandbox |
//! | [`platforms`] | Browsers, WASI, offline builds, near-kit-primitives in contracts, tracing |
//!
//! The chapters render on docs.rs with every documented feature enabled. A
//! chapter whose examples need an optional feature shows a short note
//! instead when that feature is off.

// Rendered by rustdoc and doctested; never part of a normal build.
#![cfg(all(any(doc, doctest), feature = "rpc"))]

/// The note a chapter shows when the features its examples need are off.
macro_rules! needs_features {
    ($features:literal) => {
        concat!(
            "This chapter's examples need the ",
            $features,
            " feature(s). Read it on [docs.rs](https://docs.rs/near-kit/0.30.0-rc.2), ",
            "or build the docs with those features enabled."
        )
    };
}

#[doc = include_str!("../docs/guide/clients.md")]
pub mod clients {}

#[cfg_attr(
    all(feature = "mnemonic", feature = "file-signer", feature = "keyring"),
    doc = include_str!("../docs/guide/keys.md")
)]
#[cfg_attr(
    not(all(feature = "mnemonic", feature = "file-signer", feature = "keyring")),
    doc = "# Keys and signers\n\n",
    doc = needs_features!("`mnemonic`, `file-signer` and `keyring`")
)]
pub mod keys {}

#[doc = include_str!("../docs/guide/transactions.md")]
pub mod transactions {}

#[doc = include_str!("../docs/guide/finality.md")]
pub mod finality {}

#[doc = include_str!("../docs/guide/nonces.md")]
pub mod nonces {}

#[doc = include_str!("../docs/guide/errors.md")]
pub mod errors {}

#[doc = include_str!("../docs/guide/meta_transactions.md")]
pub mod meta_transactions {}

#[doc = include_str!("../docs/guide/offline_signing.md")]
pub mod offline_signing {}

#[cfg_attr(
    feature = "contracts",
    doc = include_str!("../docs/guide/contracts_and_tokens.md")
)]
#[cfg_attr(
    not(feature = "contracts"),
    doc = "# Typed contracts and tokens\n\n",
    doc = needs_features!("`contracts`")
)]
pub mod contracts_and_tokens {}

#[cfg_attr(feature = "contracts", doc = include_str!("../docs/guide/testing.md"))]
#[cfg_attr(
    not(feature = "contracts"),
    doc = "# Testing\n\n",
    doc = needs_features!("`contracts`")
)]
pub mod testing {}

#[doc = include_str!("../docs/guide/platforms.md")]
pub mod platforms {}
