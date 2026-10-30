//! Transactions, function calls, and typed execution wait levels.
//!
//! The standalone items ([`FunctionCall`] and the wait levels) are
//! re-exported from `near-kit-primitives`; [`Tx`] and the pre-signed
//! submission need the `rpc` feature. The awaitable [`View`](crate::View)
//! and [`Call`](crate::Call) returned by `#[contract]` clients are at the
//! crate root.
//!
//! # Wait levels
//!
//! Instead of using a runtime enum, each wait level is its own type with an
//! associated `Response` type. A [`SigningNear`][signing-near] carries
//! its level as a type parameter, set once with
//! [`wait_until`][wait-until], so the compiler knows what
//! each transaction resolves to:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! # use near_kit::*;
//! # use near_kit::protocol::Action;
//! # use near_kit::transaction::{ExecutedOptimistic, Included};
//! # async fn example(alice: &SigningNear) -> Result<(), Error> {
//! let transfer = || Action::transfer(NearToken::from_near(1));
//! let bob = account!("bob.testnet");
//!
//! // Default (Final) — returns FinalExecutionOutcome
//! alice.transaction(&bob).add_action(transfer()).await?.result()?;
//!
//! // Other executed levels — also return FinalExecutionOutcome
//! alice.wait_until::<ExecutedOptimistic>().transaction(&bob).add_action(transfer()).await?.result()?;
//!
//! // Non-executed levels — returns SendTxResponse (hash + sender, plus any
//! // partial outcome the RPC has already produced)
//! let response = alice
//!     .wait_until::<Included>()
//!     .transaction(&bob)
//!     .add_action(transfer())
//!     .await?;
//! println!("tx hash: {}", response.transaction_hash);
//! # Ok(())
//! # }
//! ```
//!
#![cfg_attr(
    feature = "rpc",
    doc = "[signing-near]: crate::SigningNear
[wait-until]: crate::SigningNear::wait_until"
)]
#![cfg_attr(
    not(feature = "rpc"),
    doc = "[`Tx`]: https://docs.rs/near-kit/0.30.0-rc.3/near_kit/transaction/struct.Tx.html
[signing-near]: https://docs.rs/near-kit/0.30.0-rc.3/near_kit/struct.SigningNear.html
[wait-until]: https://docs.rs/near-kit/0.30.0-rc.3/near_kit/struct.SigningNear.html#method.wait_until"
)]

#[doc(inline)]
pub use near_kit_primitives::transaction::{CodecError, FunctionCall};

#[doc(inline)]
pub use near_kit_primitives::transaction::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, WaitLevel,
};

#[cfg(feature = "rpc")]
pub use crate::client::{DelegateOptions, SignedTransactionSend, Tx};
