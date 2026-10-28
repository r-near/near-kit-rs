//! Nonce manager for concurrent transaction handling.
//!
//! Prevents nonce collisions when sending multiple transactions in parallel
//! by caching nonces in memory and incrementing them under a lock.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::error::{Error, RpcError};
use crate::types::{AccountId, ChainId, PublicKey};

/// nearcore's `AccessKey::ACCESS_KEY_NONCE_RANGE_MULTIPLIER`: a transaction
/// included at height `h` needs a nonce below `h * 1_000_000`.
const NONCE_RANGE_MULTIPLIER: u64 = 1_000_000;

/// One access key on one chain.
///
/// Keyed by chain id rather than RPC URL, so every client for a network
/// shares one sequence however its endpoint is spelled.
type Key = (ChainId, AccountId, PublicKey);

/// Manages nonces for concurrent transactions.
///
/// Prevents nonce collisions when sending multiple transactions in parallel
/// by caching nonces in memory and incrementing them under a lock.
pub struct NonceManager {
    /// Cached nonces: value = last nonce handed out for this key.
    nonces: Mutex<HashMap<Key, u64>>,
}

impl Default for NonceManager {
    fn default() -> Self {
        Self::new()
    }
}

impl NonceManager {
    /// Create a new nonce manager.
    pub fn new() -> Self {
        Self {
            nonces: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Key, u64>> {
        // The map only holds integers, each written in one step, so a panic
        // elsewhere while the lock was held can't leave it half-updated.
        self.nonces.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Get the next nonce for an access key on a chain.
    ///
    /// Takes the key's nonce from a view at `block_height` and returns
    /// `max(cached, chain_nonce) + 1`. This handles both the initial fetch
    /// and concurrent increments: when multiple transactions race, the cached
    /// value stays ahead of the (stale) chain nonce.
    ///
    /// A cached nonce whose successor is at or above `block_height * 1e6`
    /// can't come from this chain (every nonce in the view is below that
    /// bound, and only about a million sends in flight could reach it), so
    /// the view wins: the entry came from another chain with the same id or
    /// the same endpoint.
    ///
    /// Fails, leaving the entry as it was, when no nonce follows: the node
    /// reported a nonce of `u64::MAX`.
    pub fn next(
        &self,
        chain_id: &ChainId,
        account_id: &AccountId,
        public_key: &PublicKey,
        chain_nonce: u64,
        block_height: u64,
    ) -> Result<u64, Error> {
        let bound = block_height.saturating_mul(NONCE_RANGE_MULTIPLIER);
        let mut nonces = self.lock();
        let key = (chain_id.clone(), account_id.clone(), public_key.clone());
        let current = match nonces.get(&key) {
            Some(&cached) if cached.saturating_add(1) < bound => cached.max(chain_nonce),
            _ => chain_nonce,
        };
        let next = current
            .checked_add(1)
            .ok_or_else(|| no_next_nonce(current))?;
        nonces.insert(key, next);
        Ok(next)
    }

    /// Drop the cached nonce for an access key, so the next call starts from
    /// the chain's view again. Used once the node rejects a cached nonce as
    /// too large.
    pub fn forget(&self, chain_id: &ChainId, account_id: &AccountId, public_key: &PublicKey) {
        self.lock()
            .remove(&(chain_id.clone(), account_id.clone(), public_key.clone()));
    }
}

/// The error for a node-reported nonce of `u64::MAX`, which no nonce follows.
pub(super) fn no_next_nonce(nonce: u64) -> Error {
    RpcError::InvalidResponse(format!("access key nonce {nonce} has no next nonce")).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEIGHT: u64 = 100;

    fn alice() -> AccountId {
        "alice.testnet".parse().unwrap()
    }

    fn alice_key() -> PublicKey {
        "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap()
    }

    fn next(manager: &NonceManager, chain_nonce: u64) -> u64 {
        manager
            .next(
                &ChainId::testnet(),
                &alice(),
                &alice_key(),
                chain_nonce,
                HEIGHT,
            )
            .unwrap()
    }

    #[test]
    fn test_nonce_manager_caching() {
        let manager = NonceManager::new();
        // First call: blockchain nonce is 10, should return 11
        assert_eq!(next(&manager, 10), 11);
        // Second call: blockchain nonce still 10 (stale), cached is 11
        assert_eq!(next(&manager, 10), 12);
        assert_eq!(next(&manager, 10), 13);
    }

    #[test]
    fn test_chain_catches_up() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 10), 11);
        // Chain advanced past our cache (e.g. another client sent txs)
        assert_eq!(next(&manager, 15), 16); // max(11, 15) + 1
    }

    #[test]
    fn test_different_keys() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 10), 11);

        let bob: AccountId = "bob.testnet".parse().unwrap();
        let bob_key: PublicKey = "ed25519:22skMptHjFWNyuEWY22ftn2AbLPSYpmYwGJRGwpNHbTV"
            .parse()
            .unwrap();
        let nonce_bob = manager
            .next(&ChainId::testnet(), &bob, &bob_key, 20, HEIGHT)
            .unwrap();
        assert_eq!(nonce_bob, 21);

        // Alice's nonce should still increment from cache
        assert_eq!(next(&manager, 10), 12);
    }

    #[test]
    fn test_different_chains() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 10), 11);

        // Same account+key on mainnet should be separate
        let nonce_mainnet = manager
            .next(&ChainId::mainnet(), &alice(), &alice_key(), 50, HEIGHT)
            .unwrap();
        assert_eq!(nonce_mainnet, 51);

        // Testnet should still increment from its own cache
        assert_eq!(next(&manager, 10), 12);
    }

    #[test]
    fn test_ak_nonce_from_error() {
        let manager = NonceManager::new();
        // Simulate: sent tx with nonce 11, got InvalidNonce with ak_nonce=100
        assert_eq!(next(&manager, 10), 11);
        // Retry with ak_nonce from error (higher than cache)
        assert_eq!(next(&manager, 100), 101);
        // Subsequent call, chain still stale
        assert_eq!(next(&manager, 100), 102);
    }

    #[test]
    fn test_lower_ak_nonce_uses_cache() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 100), 101);
        assert_eq!(next(&manager, 100), 102);
        assert_eq!(next(&manager, 100), 103);
        // Lower chain nonce: cache should win
        assert_eq!(next(&manager, 90), 104); // max(103, 90) + 1
    }

    #[test]
    fn cached_nonce_past_the_height_bound_falls_back_to_the_view() {
        let manager = NonceManager::new();
        let chain = ChainId::new("sandbox");
        // Cached on a chain at height 1095...
        let tall = manager
            .next(&chain, &alice(), &alice_key(), 1_086_000_000, 1095)
            .unwrap();
        assert_eq!(tall, 1_086_000_001);
        // ...then another chain with the same id, at height 122: the cached
        // nonce can never land there.
        let short = manager
            .next(&chain, &alice(), &alice_key(), 121_000_000, 122)
            .unwrap();
        assert_eq!(short, 121_000_001);
        // The reset entry keeps counting from the view.
        let again = manager
            .next(&chain, &alice(), &alice_key(), 121_000_000, 122)
            .unwrap();
        assert_eq!(again, 121_000_002);
    }

    #[test]
    fn cached_nonce_just_below_the_bound_is_kept() {
        let manager = NonceManager::new();
        let chain = ChainId::new("bound-edge");
        // 99_999_998 + 1 is still below 100 * 1e6.
        let first = manager
            .next(&chain, &alice(), &alice_key(), 99_999_997, HEIGHT)
            .unwrap();
        assert_eq!(first, 99_999_998);
        let second = manager
            .next(&chain, &alice(), &alice_key(), 5, HEIGHT)
            .unwrap();
        assert_eq!(second, 99_999_999);
        // Now the successor would reach the bound: back to the view.
        let third = manager
            .next(&chain, &alice(), &alice_key(), 5, HEIGHT)
            .unwrap();
        assert_eq!(third, 6);
    }

    #[test]
    fn forget_starts_again_from_the_view() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 10), 11);
        assert_eq!(next(&manager, 10), 12);
        manager.forget(&ChainId::testnet(), &alice(), &alice_key());
        assert_eq!(next(&manager, 10), 11);
    }

    #[test]
    fn u64_max_is_an_error_and_leaves_no_entry() {
        let manager = NonceManager::new();
        let error = manager
            .next(
                &ChainId::testnet(),
                &alice(),
                &alice_key(),
                u64::MAX,
                HEIGHT,
            )
            .unwrap_err();
        assert!(
            matches!(&error, Error::Rpc(e) if matches!(**e, RpcError::InvalidResponse(_))),
            "{error:?}"
        );
        // Nothing was cached, so a sane view works at once.
        assert_eq!(next(&manager, 10), 11);
    }
}
