//! Platform-conditional trait bounds for wasm32 compatibility.
//!
//! On native targets, futures must be `Send` and trait objects must be `Send + Sync`.
//! On wasm32, there are no threads, so these bounds are unnecessary (and often
//! impossible to satisfy with browser APIs).

use std::future::Future;
use std::pin::Pin;

/// A boxed future that is `Send` on native and `!Send` on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// Trait alias: `Send` on native, unconditional on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub(crate) trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSend for T {}

/// Trait alias: `Sync` on native, unconditional on wasm32.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync> MaybeSync for T {}

#[cfg(target_arch = "wasm32")]
pub(crate) trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T> MaybeSync for T {}
