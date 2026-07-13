//! Platform-conditional trait bounds for JS-host wasm compatibility.
//!
//! On most targets, futures must be `Send` and trait objects must be `Send + Sync`.
//! On `wasm32-unknown-unknown` (browsers and other JS hosts), there are no threads,
//! so these bounds are unnecessary (and often impossible to satisfy with browser
//! APIs). WASI targets (`wasm32-wasip1`/`p2`) keep the regular bounds.

use std::future::Future;
use std::pin::Pin;

/// A boxed future that is `Send` everywhere except `wasm32-unknown-unknown`.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Trait alias: `Send` everywhere except `wasm32-unknown-unknown`, where it's
/// unconditional.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) trait MaybeSend: Send {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Send> MaybeSend for T {}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) trait MaybeSend {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> MaybeSend for T {}

/// Trait alias: `Sync` everywhere except `wasm32-unknown-unknown`, where it's
/// unconditional.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) trait MaybeSync: Sync {}
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
impl<T: Sync> MaybeSync for T {}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) trait MaybeSync {}
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
impl<T> MaybeSync for T {}
