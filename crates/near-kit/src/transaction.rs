//! Fluent transaction construction and typed execution wait levels.
//!
//! The standalone items ([`FunctionCall`] and the wait levels) are re-exported
//! from `near-kit-primitives`; the builders need the `rpc` feature.
//!
//! # Wait levels
//!
//! Instead of using a runtime enum, each wait level is its own type with an
//! associated `Response` type. This lets the compiler determine the return type
//! of `send().wait_until::<W>()` from the selected type:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! # use near_kit::*;
//! # use near_kit::transaction::{Final, Included};
//! # async fn example(near: &Near) -> Result<(), Error> {
//! // Default — returns FinalExecutionOutcome
//! near.transfer("bob.testnet", NearToken::from_near(1)).await?;
//!
//! // Executed levels — also returns FinalExecutionOutcome
//! near.transfer("bob.testnet", NearToken::from_near(1))
//!     .wait_until::<Final>()
//!     .await?;
//!
//! // Non-executed levels — returns SendTxResponse (hash + sender, plus any
//! // partial outcome the RPC has already produced)
//! let response = near.transfer("bob.testnet", NearToken::from_near(1))
//!     .wait_until::<Included>()
//!     .await?;
//! println!("tx hash: {}", response.transaction_hash);
//! # Ok(())
//! # }
//! ```

#[doc(inline)]
pub use near_kit_primitives::transaction::{FunctionCall, FunctionCallBuildError};

#[doc(inline)]
pub use near_kit_primitives::transaction::{
    Executed, ExecutedOptimistic, Final, Included, IncludedFinal, Submitted, WaitLevel,
};

#[cfg(feature = "rpc")]
pub use crate::client::{
    CallBuilder, DelegateOptions, DelegateResult, SignedTransactionSend, TransactionBuilder,
    TransactionSend,
};
