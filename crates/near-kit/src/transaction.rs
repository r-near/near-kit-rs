//! Transactions, contract calls and views, and typed execution wait levels.
//!
//! The standalone items ([`FunctionCall`], the awaitable [`View`] and
//! [`Call`] returned by `#[contract]` clients, and the wait levels) are
//! re-exported from `near-kit-primitives`; [`Tx`] and the pre-signed
//! submission need the `rpc` feature.
//!
//! # Wait levels
//!
//! Instead of using a runtime enum, each wait level is its own type with an
//! associated `Response` type. A [`SigningNear`](crate::SigningNear) carries
//! its level as a type parameter, set once with
//! [`wait_until`](crate::SigningNear::wait_until), so the compiler knows what
//! each transaction resolves to:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! # use near_kit::*;
//! # use near_kit::protocol::Action;
//! # use near_kit::transaction::{Final, Included};
//! # async fn example(alice: &SigningNear) -> Result<(), Error> {
//! let transfer = || Action::transfer(NearToken::from_near(1));
//! let bob = account!("bob.testnet");
//!
//! // Default (ExecutedOptimistic) — returns FinalExecutionOutcome
//! alice.transaction(&bob).add_action(transfer()).await?;
//!
//! // Executed levels — also returns FinalExecutionOutcome
//! alice.wait_until::<Final>().transaction(&bob).add_action(transfer()).await?;
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

#[doc(inline)]
pub use near_kit_primitives::transaction::{Call, CodecError, ContractError, FunctionCall, View};

#[doc(inline)]
pub use near_kit_primitives::transaction::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, WaitLevel,
};

#[cfg(feature = "rpc")]
pub use crate::client::{DelegateOptions, DelegateResult, SignedTransactionSend, Tx};
