//! Nonce manager for concurrent transaction handling.
//!
//! Prevents nonce collisions when sending multiple transactions in parallel
//! by caching nonces in memory and incrementing them under a lock.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::error::{Error, RpcError};
use crate::trace;
use crate::types::{AccountId, ChainId, CryptoHash, PublicKey};

use super::rpc::RpcClient;

/// The chain a client draws nonces for: its configured chain id, and the
/// genesis hash its node reported, when it has.
#[derive(Clone, Debug)]
pub(super) struct Chain {
    id: ChainId,
    genesis: Option<CryptoHash>,
}

impl Chain {
    fn key(&self, account_id: &AccountId, public_key: &PublicKey) -> Key {
        let chain = match self.genesis {
            Some(hash) => ChainKey::Genesis(hash),
            None => ChainKey::Id(self.id.clone()),
        };
        (chain, account_id.clone(), public_key.clone())
    }

    /// The chain-id key a draw made before the genesis hash was known used.
    fn fallback_key(&self, account_id: &AccountId, public_key: &PublicKey) -> Option<Key> {
        self.genesis.is_some().then(|| {
            (
                ChainKey::Id(self.id.clone()),
                account_id.clone(),
                public_key.clone(),
            )
        })
    }
}

/// A client's genesis hash, fetched on its first nonce draw and kept for the
/// client and its clones.
#[derive(Debug, Default)]
pub(super) struct GenesisHash(Mutex<Option<CryptoHash>>);

impl GenesisHash {
    fn lock(&self) -> MutexGuard<'_, Option<CryptoHash>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The chain `rpc` serves. Asks the node for its genesis hash until it
    /// answers once; until then, and whenever `status` fails, the chain is
    /// known only by `chain_id`.
    pub(super) async fn chain(&self, rpc: &RpcClient, chain_id: &ChainId) -> Chain {
        let cached = *self.lock();
        let genesis = match cached {
            Some(hash) => Some(hash),
            None => match rpc.status().await {
                Ok(status) => {
                    *self.lock() = Some(status.genesis_hash);
                    Some(status.genesis_hash)
                }
                Err(_error) => {
                    trace::debug!(
                        error = %_error,
                        "No genesis hash from status; caching nonces by chain id"
                    );
                    None
                }
            },
        };
        Chain {
            id: chain_id.clone(),
            genesis,
        }
    }
}

/// Which chain an entry belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ChainKey {
    /// The chain's genesis hash: one per chain, however its RPC URL or chain
    /// id is spelled, and distinct for two chains that share a chain id (two
    /// local sandboxes, say).
    Genesis(CryptoHash),
    /// The configured chain id, for a node that hasn't reported its genesis.
    Id(ChainId),
}

/// One access key on one chain.
type Key = (ChainKey, AccountId, PublicKey);

/// Manages nonces for concurrent transactions.
///
/// Prevents nonce collisions when sending multiple transactions in parallel
/// by caching nonces in memory and incrementing them under a lock.
pub struct NonceManager {
    /// Per key, the last nonce handed out.
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

    fn lock(&self) -> MutexGuard<'_, HashMap<Key, u64>> {
        // The map only holds integers, each written in one step, so a panic
        // elsewhere while the lock was held can't leave it half-updated.
        self.nonces.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Get the next nonce for an access key on a chain.
    ///
    /// Takes the key's nonce from a view (or a rejection) and returns
    /// `max(cached, chain_nonce) + 1`. This handles both the initial fetch
    /// and concurrent increments: when multiple transactions race, the cached
    /// value stays ahead of the (stale) chain nonce, and a view older than
    /// an earlier one (a final view after an optimistic one) never takes the
    /// sequence back.
    ///
    /// A key's first draw once the genesis hash is known continues from what
    /// draws made by chain id handed out, while `status` was failing.
    ///
    /// Fails, leaving the entry as it was, when no nonce follows: the node
    /// reported a nonce of `u64::MAX`.
    pub(super) fn next(
        &self,
        chain: &Chain,
        account_id: &AccountId,
        public_key: &PublicKey,
        chain_nonce: u64,
    ) -> Result<u64, Error> {
        let key = chain.key(account_id, public_key);
        let mut nonces = self.lock();
        let cached = nonces.get(&key).copied().or_else(|| {
            chain
                .fallback_key(account_id, public_key)
                .and_then(|fallback| nonces.get(&fallback).copied())
        });
        let current = cached.map_or(chain_nonce, |cached| cached.max(chain_nonce));
        let next = current
            .checked_add(1)
            .ok_or_else(|| no_next_nonce(current))?;
        nonces.insert(key, next);
        Ok(next)
    }

    /// Drop the cached nonce for an access key once the node rejected
    /// `tx_nonce` as too large, so the next call starts from the chain's view
    /// again. Only this chain's entry goes, and the chain-id entry a first
    /// draw would continue from, if it is as high.
    ///
    /// Nothing is dropped when the entry is already below `tx_nonce`: another
    /// send's rejection reset it and nonces were handed out from the view
    /// since (all of them below the rejected one), so dropping it would let
    /// the next send draw one of those again.
    pub(super) fn forget_rejected(
        &self,
        chain: &Chain,
        account_id: &AccountId,
        public_key: &PublicKey,
        tx_nonce: u64,
    ) {
        let mut nonces = self.lock();
        let keys = [
            Some(chain.key(account_id, public_key)),
            chain.fallback_key(account_id, public_key),
        ];
        for key in keys.into_iter().flatten() {
            if nonces.get(&key).is_some_and(|&cached| cached >= tx_nonce) {
                nonces.remove(&key);
            }
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

    fn alice() -> AccountId {
        "alice.testnet".parse().unwrap()
    }

    fn alice_key() -> PublicKey {
        "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap()
    }

    /// A chain whose node reported genesis hash `[byte; 32]`.
    fn genesis(id: &str, byte: u8) -> Chain {
        Chain {
            id: ChainId::new(id),
            genesis: Some(CryptoHash::from_bytes([byte; 32])),
        }
    }

    /// A chain known only by its id: its node didn't answer `status`.
    fn by_id(id: &str) -> Chain {
        Chain {
            id: ChainId::new(id),
            genesis: None,
        }
    }

    fn testnet() -> Chain {
        genesis("testnet", 1)
    }

    fn draw(manager: &NonceManager, chain: &Chain, chain_nonce: u64) -> u64 {
        manager
            .next(chain, &alice(), &alice_key(), chain_nonce)
            .unwrap()
    }

    fn next(manager: &NonceManager, chain_nonce: u64) -> u64 {
        draw(manager, &testnet(), chain_nonce)
    }

    fn forget_rejected(manager: &NonceManager, tx_nonce: u64) {
        manager.forget_rejected(&testnet(), &alice(), &alice_key(), tx_nonce);
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
        let nonce_bob = manager.next(&testnet(), &bob, &bob_key, 20).unwrap();
        assert_eq!(nonce_bob, 21);

        // Alice's nonce should still increment from cache
        assert_eq!(next(&manager, 10), 12);
    }

    #[test]
    fn test_different_chains() {
        let manager = NonceManager::new();
        assert_eq!(next(&manager, 10), 11);

        // Same account+key on mainnet should be separate
        assert_eq!(draw(&manager, &genesis("mainnet", 2), 50), 51);

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
    fn two_chains_with_one_id_keep_separate_sequences() {
        // Two sandboxes: both "sandbox", same root key, distinct genesis. The
        // shorter chain draws first. When sequences were told apart by the
        // height bound alone, the taller chain's draw lifted the short one's
        // and the short chain's next draw started over at 121_000_001.
        let manager = NonceManager::new();
        let short = genesis("sandbox", 3);
        let tall = genesis("sandbox", 4);
        assert_eq!(draw(&manager, &short, 121_000_000), 121_000_001);
        assert_eq!(draw(&manager, &tall, 1_086_000_000), 1_086_000_001);
        assert_eq!(draw(&manager, &short, 121_000_000), 121_000_002);
        assert_eq!(draw(&manager, &tall, 1_086_000_000), 1_086_000_002);
    }

    #[test]
    fn rejection_drops_only_its_own_chain() {
        let manager = NonceManager::new();
        let short = genesis("sandbox", 3);
        let tall = genesis("sandbox", 4);
        assert_eq!(draw(&manager, &short, 121_000_000), 121_000_001);
        assert_eq!(draw(&manager, &tall, 1_086_000_000), 1_086_000_001);
        // A rejection on the tall chain, of a nonce below the short chain's
        // entry, leaves the short chain's alone.
        manager.forget_rejected(&tall, &alice(), &alice_key(), 1_086_000_001);
        assert_eq!(draw(&manager, &short, 121_000_000), 121_000_002);
        assert_eq!(draw(&manager, &tall, 5), 6);
        // And the other way round.
        manager.forget_rejected(&short, &alice(), &alice_key(), 121_000_002);
        assert_eq!(draw(&manager, &tall, 5), 7);
        assert_eq!(draw(&manager, &short, 7), 8);
    }

    #[test]
    fn one_genesis_is_one_sequence_whatever_the_chain_id() {
        // Clients for one chain (however their URLs or ids are spelled) all
        // see its genesis hash.
        let manager = NonceManager::new();
        assert_eq!(draw(&manager, &genesis("mainnet", 5), 10), 11);
        assert_eq!(draw(&manager, &genesis("near-mainnet", 5), 10), 12);
    }

    #[test]
    fn an_older_view_keeps_a_newer_reservation() {
        let manager = NonceManager::new();
        let chain = genesis("older-view", 6);
        // A delegate drew from an optimistic view after the key was re-added
        // at height 102.
        assert_eq!(draw(&manager, &chain, 102_000_000), 102_000_001);
        // A send reads a final view that still shows the old key.
        assert_eq!(draw(&manager, &chain, 50), 102_000_002);
        // The next delegate continues past both.
        assert_eq!(draw(&manager, &chain, 102_000_000), 102_000_003);
    }

    #[test]
    fn draws_by_chain_id_carry_over_once_the_genesis_is_known() {
        let manager = NonceManager::new();
        // `status` failed: the draw is cached by chain id.
        assert_eq!(draw(&manager, &by_id("recovers"), 10), 11);
        // It answered: the same view doesn't hand 11 out again.
        let known = genesis("recovers", 7);
        assert_eq!(draw(&manager, &known, 10), 12);
        assert_eq!(draw(&manager, &known, 10), 13);
        // A rejection as high as the chain-id entry drops it too, so the next
        // draw starts from the view rather than from that entry.
        manager.forget_rejected(&known, &alice(), &alice_key(), 11);
        assert_eq!(draw(&manager, &known, 10), 11);
    }

    #[test]
    fn chains_known_only_by_id_share_by_id() {
        let manager = NonceManager::new();
        assert_eq!(draw(&manager, &by_id("no-status"), 10), 11);
        assert_eq!(draw(&manager, &by_id("no-status"), 10), 12);
        assert_eq!(draw(&manager, &by_id("other"), 10), 11);
    }

    #[test]
    fn u64_max_is_an_error_and_leaves_no_entry() {
        let manager = NonceManager::new();
        let error = manager
            .next(&testnet(), &alice(), &alice_key(), u64::MAX)
            .unwrap_err();
        assert!(
            matches!(&error, Error::Rpc(e) if matches!(**e, RpcError::InvalidResponse(_))),
            "{error:?}"
        );
        // Nothing was cached, so a sane view works at once.
        assert_eq!(next(&manager, 10), 11);
    }
}
