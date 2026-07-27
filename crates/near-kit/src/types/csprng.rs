//! The OS CSPRNG, adapted to `rand_core` 0.10's infallible traits.
//!
//! `rand` 0.10 renamed `OsRng` to [`SysRng`] and, more importantly, made it
//! *fallible*: it implements only [`TryRng`]/[`TryCryptoRng`], not the
//! infallible [`Rng`]/[`CryptoRng`] that `SigningKey::generate` and
//! `SecretKey::random` require. Every near-kit caller is an infallible
//! `pub fn` returning a key, a seed phrase, or a nonce — not a `Result` — so
//! there is nowhere to report an entropy failure without breaking the public
//! API.
//!
//! [`UnwrapErr`] is therefore a deliberate choice, not an oversight: a failing
//! OS entropy source panics here, which is exactly what `rand` 0.8's `OsRng`
//! and `ed25519_dalek::SigningKey::generate` did before this bump. The failure
//! is never swallowed and never falls back to a weaker source.
//!
//! [`Rng`]: rand::Rng
//! [`CryptoRng`]: rand::CryptoRng
//! [`TryRng`]: rand::TryRng
//! [`TryCryptoRng`]: rand::TryCryptoRng

use rand::Rng as _;
use rand::rand_core::UnwrapErr;
use rand::rngs::SysRng;

/// The operating system's CSPRNG as an infallible [`CryptoRng`], for the
/// keygen APIs that are generic over it. Panics if the OS source fails.
///
/// [`CryptoRng`]: rand::CryptoRng
pub(crate) fn os_csprng() -> UnwrapErr<SysRng> {
    UnwrapErr(SysRng)
}

/// Fill `dst` with bytes from the operating system's CSPRNG.
///
/// Panics if the OS source fails; see the module docs.
pub(crate) fn fill_random(dst: &mut [u8]) {
    os_csprng().fill_bytes(dst);
}
