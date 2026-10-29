//! The OS CSPRNG, adapted to `rand_core` 0.10's infallible traits.
//!
//! [`SysRng`] only implements the fallible `TryRng`/`TryCryptoRng`, and every
//! caller is an infallible `pub fn` with nowhere to report an entropy failure.
//! So a failing OS source panics (as `rand` 0.8's `OsRng` did) rather than
//! falling back to a weaker one.
//!
//! [`SysRng`]: getrandom::SysRng

use getrandom::SysRng;
use getrandom::rand_core::{Rng as _, UnwrapErr};

/// The operating system's CSPRNG as an infallible [`CryptoRng`], for the
/// keygen APIs that are generic over it. Panics if the OS source fails.
///
/// [`CryptoRng`]: getrandom::rand_core::CryptoRng
pub(crate) fn os_csprng() -> UnwrapErr<SysRng> {
    UnwrapErr(SysRng)
}

/// Fill `dst` with bytes from the operating system's CSPRNG.
///
/// Panics if the OS source fails; see the module docs.
pub(crate) fn fill_random(dst: &mut [u8]) {
    os_csprng().fill_bytes(dst);
}
