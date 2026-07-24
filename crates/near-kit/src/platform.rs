//! Platform-conditional trait bounds for JS-host wasm compatibility.
//!
//! On most targets, futures must be `Send` and trait objects must be `Send + Sync`.
//! On `wasm32-unknown-unknown` (browsers and other JS hosts), there are no threads,
//! so these bounds are unnecessary (and often impossible to satisfy with browser
//! APIs). WASI targets (`wasm32-wasip1`/`p2`) keep the regular bounds.
//!
//! The items here are `pub` (not `pub(crate)`) because they appear in the
//! public [`RpcTransport`](crate::client::RpcTransport) interface — `BoxFuture`
//! is re-exported from there; `MaybeSend`/`MaybeSync` stay unnameable outside
//! the crate (their blanket impls apply to every eligible type, so implementors
//! never spell them out).

use std::future::Future;
use std::pin::Pin;

/// A boxed future that is `Send` everywhere except `wasm32-unknown-unknown`.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Trait alias: `Send` everywhere except `wasm32-unknown-unknown`, where it's
/// unconditional.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub trait MaybeSend: Send {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Send> MaybeSend for T {}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait MaybeSend {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> MaybeSend for T {}

/// Trait alias: `Sync` everywhere except `wasm32-unknown-unknown`, where it's
/// unconditional.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub trait MaybeSync: Sync {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Sync> MaybeSync for T {}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub trait MaybeSync {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> MaybeSync for T {}
