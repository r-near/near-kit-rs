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

/// Most reservations kept per key; past this the smallest is dropped.
const MAX_SEQUENCES: usize = 8;

/// Manages nonces for concurrent transactions.
///
/// Prevents nonce collisions when sending multiple transactions in parallel
/// by caching nonces in memory and incrementing them under a lock.
pub struct NonceManager {
    /// Per key, the last nonce handed out by each live sequence (see
    /// [`next`](Self::next)).
    nonces: Mutex<HashMap<Key, Vec<u64>>>,
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

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<Key, Vec<u64>>> {
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
    /// A key can hold several sequences. One whose successor is at or above
    /// the view's bound (`block_height * 1e6`, or higher when `chain_nonce`
    /// shows the chain is past `block_height`) can't land at this view, so
    /// it is skipped, and a new sequence starts from the view when no other
    /// fits. It isn't dropped: it may come from another chain with the same
    /// id (two sandboxes), or from a newer view of this one than the caller
    /// read (a delegate's optimistic view ahead of a send's final view, after
    /// the key was re-added). Dropping it in the second case would hand its
    /// nonces out again.
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
        // The chain is at least at the height its nonce implies: a nonce
        // from a rejection can be ahead of a trailing view.
        let implied_height = (chain_nonce / NONCE_RANGE_MULTIPLIER).saturating_add(1);
        let bound = block_height
            .max(implied_height)
            .saturating_mul(NONCE_RANGE_MULTIPLIER);
        let mut nonces = self.lock();
        let key = (chain_id.clone(), account_id.clone(), public_key.clone());
        let sequences = nonces.entry(key.clone()).or_default();
        let fitting = sequences
            .iter()
            .enumerate()
            .filter(|&(_, &cached)| cached.saturating_add(1) < bound)
            .max_by_key(|&(_, &cached)| cached)
            .map(|(index, &cached)| (index, cached));
        let current = fitting.map_or(chain_nonce, |(_, cached)| cached.max(chain_nonce));
        let Some(next) = current.checked_add(1) else {
            if sequences.is_empty() {
                nonces.remove(&key);
            }
            return Err(no_next_nonce(current));
        };
        match fitting {
            Some((index, _)) => sequences[index] = next,
            None => {
                if sequences.len() >= MAX_SEQUENCES
                    && let Some(smallest) = sequences
                        .iter()
                        .enumerate()
                        .min_by_key(|&(_, &cached)| cached)
                        .map(|(index, _)| index)
                {
                    sequences.swap_remove(smallest);
                }
                sequences.push(next);
            }
        }
        Ok(next)
    }

    /// Drop the cached nonce for an access key once the node rejected
    /// `tx_nonce` as too large, so the next call starts from the chain's view
    /// again.
    ///
    /// Only the sequence that handed `tx_nonce` out is dropped: the lowest one
    /// at or above it. When every sequence is lower, another send's rejection
    /// already reset it and nonces were handed out from the view since (all
    /// of them below the rejected one); dropping one would let the next send
    /// draw one of those again.
    pub fn forget_rejected(
        &self,
        chain_id: &ChainId,
        account_id: &AccountId,
        public_key: &PublicKey,
        tx_nonce: u64,
    ) {
        let key = (chain_id.clone(), account_id.clone(), public_key.clone());
        let mut nonces = self.lock();
        let Some(sequences) = nonces.get_mut(&key) else {
            return;
        };
        if let Some(index) = sequences
            .iter()
            .enumerate()
            .filter(|&(_, &cached)| cached >= tx_nonce)
            .min_by_key(|&(_, &cached)| cached)
            .map(|(index, _)| index)
        {
            sequences.swap_remove(index);
        }
        if sequences.is_empty() {
            nonces.remove(&key);
        }
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

    fn forget_rejected(manager: &NonceManager, tx_nonce: u64) {
        manager.forget_rejected(&ChainId::testnet(), &alice(), &alice_key(), tx_nonce);
    }

    #[test]
    fn test_rejection_resets_the_entry() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 500), 501);
        assert_eq!(next(&manager, 500), 502);
        // 502 was rejected as too large: start again from the view.
        forget_rejected(&manager, 502);
        assert_eq!(next(&manager, 10), 11);
    }

    #[test]
    fn test_stale_rejection_keeps_newer_reservations() {
        let manager = NonceManager::new();
        // Two sends drew 501 and 502 from a cache the chain can't accept.
        assert_eq!(next(&manager, 500), 501);
        assert_eq!(next(&manager, 500), 502);
        // The first rejection resets the entry and its retry takes 11.
        forget_rejected(&manager, 501);
        assert_eq!(next(&manager, 10), 11);
        // The second rejection arrives late. Dropping the entry now would
        // hand 11 out again; it stays, so the second retry takes 12.
        forget_rejected(&manager, 502);
        assert_eq!(next(&manager, 10), 12);
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
    fn an_older_view_keeps_a_newer_reservation() {
        let manager = NonceManager::new();
        let chain = ChainId::new("older-view");
        // A delegate drew from an optimistic view at 103, after the key was
        // re-added at 102.
        let delegate = manager
            .next(&chain, &alice(), &alice_key(), 102_000_000, 103)
            .unwrap();
        assert_eq!(delegate, 102_000_001);
        // A send reads a final view at 100, which still shows the old key.
        let send = manager
            .next(&chain, &alice(), &alice_key(), 50, 100)
            .unwrap();
        assert_eq!(send, 51);
        // The next delegate continues past the first one.
        let again = manager
            .next(&chain, &alice(), &alice_key(), 102_000_000, 103)
            .unwrap();
        assert_eq!(again, 102_000_002);
        // And the send's retry, with the rejection's nonce against the same
        // trailing view, continues past both.
        let retry = manager
            .next(&chain, &alice(), &alice_key(), 102_000_000, 100)
            .unwrap();
        assert_eq!(retry, 102_000_003);
    }

    #[test]
    fn two_chains_with_one_id_keep_separate_sequences() {
        let manager = NonceManager::new();
        let chain = ChainId::new("two-sandboxes");
        let next_at = |nonce, height| {
            manager
                .next(&chain, &alice(), &alice_key(), nonce, height)
                .unwrap()
        };
        assert_eq!(next_at(1_086_000_000, 1095), 1_086_000_001);
        assert_eq!(next_at(121_000_000, 122), 121_000_001);
        assert_eq!(next_at(1_086_000_000, 1095), 1_086_000_002);
        assert_eq!(next_at(121_000_000, 122), 121_000_002);
    }

    #[test]
    fn rejection_drops_only_the_sequence_it_came_from() {
        let manager = NonceManager::new();
        let chain = ChainId::new("two-rejections");
        let next_at = |nonce, height| {
            manager
                .next(&chain, &alice(), &alice_key(), nonce, height)
                .unwrap()
        };
        assert_eq!(next_at(1_086_000_000, 1095), 1_086_000_001);
        assert_eq!(next_at(121_000_000, 122), 121_000_001);
        // 121_000_001 was rejected: only the short chain's sequence goes.
        manager.forget_rejected(&chain, &alice(), &alice_key(), 121_000_001);
        assert_eq!(next_at(121_000_000, 122), 121_000_001);
        assert_eq!(next_at(1_086_000_000, 1095), 1_086_000_002);
    }

    #[test]
    fn sequences_per_key_are_capped() {
        let manager = NonceManager::new();
        let chain = ChainId::new("many-chains");
        // Each view is too short for every earlier sequence.
        for height in (1..=20u64).rev() {
            let nonce = (height - 1) * NONCE_RANGE_MULTIPLIER;
            assert_eq!(
                manager
                    .next(&chain, &alice(), &alice_key(), nonce, height)
                    .unwrap(),
                nonce + 1
            );
        }
        let nonces = manager.lock();
        let key = (chain.clone(), alice(), alice_key());
        assert_eq!(nonces[&key].len(), MAX_SEQUENCES);
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
