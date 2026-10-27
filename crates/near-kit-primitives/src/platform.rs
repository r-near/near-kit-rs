//! Platform-conditional trait bounds for JS-host wasm compatibility.
//!
//! On most targets, futures must be `Send` and trait objects must be `Send + Sync`.
//! On `wasm32-unknown-unknown` (browsers and other JS hosts), there are no threads,
//! so these bounds are unnecessary (and often impossible to satisfy with browser
//! APIs). WASI targets (`wasm32-wasip1`/`p2`) keep the regular bounds.
//!
//! `MaybeSend` and `MaybeSync` are public at the crate root because they
//! appear in the [`Viewer`](crate::Viewer) and [`TxSender`](crate::TxSender)
//! signatures; `near-kit` re-exports them rather than defining its own.

use std::future::Future;
use std::pin::Pin;

/// A boxed future that is `Send` everywhere except `wasm32-unknown-unknown`.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// `Send` everywhere except `wasm32-unknown-unknown`, where every type
/// implements it.
///
/// Use it where a [`Viewer`](crate::Viewer) or [`TxSender`](crate::TxSender)
/// forwarding impl spells out its return type
/// (`-> impl Future<Output = ...> + MaybeSend`), or to bound a backend error
/// held across an `.await`. Writing `Send` there instead fails to build for
/// JS-host wasm. It has a blanket impl; never implement it by hand.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub trait MaybeSend: Send {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Send> MaybeSend for T {}

/// Implemented by every type on `wasm32-unknown-unknown`; `Send` elsewhere.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait MaybeSend {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> MaybeSend for T {}

/// `Sync` everywhere except `wasm32-unknown-unknown`, where every type
/// implements it. See [`MaybeSend`].
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub trait MaybeSync: Sync {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Sync> MaybeSync for T {}

/// Implemented by every type on `wasm32-unknown-unknown`; `Sync` elsewhere.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait MaybeSync {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> MaybeSync for T {}
