//! The OS CSPRNG, adapted to `rand_core` 0.10's infallible traits.
//!
//! [`SysRng`] is fallible: it implements only [`TryRng`]/[`TryCryptoRng`], not the
//! infallible [`Rng`]/[`CryptoRng`] that `SigningKey::generate` and
//! `k256::SecretKey::generate_from_rng` require. Every near-kit caller is an infallible
//! `pub fn` returning a key, a seed phrase, or a nonce — not a `Result` — so
//! there is nowhere to report an entropy failure without breaking the public
//! API.
//!
//! [`UnwrapErr`] is therefore a deliberate choice, not an oversight: a failing
//! OS entropy source panics here, which is exactly what `rand` 0.8's `OsRng`
//! and `ed25519_dalek::SigningKey::generate` did before this bump. The failure
//! is never swallowed and never falls back to a weaker source.
//!
//! [`Rng`]: getrandom::rand_core::Rng
//! [`CryptoRng`]: getrandom::rand_core::CryptoRng
//! [`TryRng`]: getrandom::rand_core::TryRng
//! [`TryCryptoRng`]: getrandom::rand_core::TryCryptoRng
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
