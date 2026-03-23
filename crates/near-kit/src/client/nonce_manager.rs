//! Nonce manager for concurrent transaction handling.
//!
//! Prevents nonce collisions when sending multiple transactions in parallel
//! by caching nonces in memory and incrementing them under a lock.

use std::{collections::HashMap, sync::Mutex};

use crate::PublicKey;
use crate::types::AccountId;

type Network = String;

/// Manages nonces for concurrent transactions.
///
/// Prevents nonce collisions when sending multiple transactions in parallel
/// by caching nonces in memory and incrementing them under a lock.
pub struct NonceManager {
    /// Cached nonces: value = last nonce handed out for this key.
    nonces: Mutex<HashMap<(Network, AccountId, PublicKey), u64>>,
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

    /// Get the next nonce for an account and public key on a specific network.
    ///
    /// Takes the current blockchain nonce (from `view_access_key`) and returns
    /// `max(cached, blockchain_nonce) + 1`. This handles both the initial fetch
    /// and concurrent increments: when multiple transactions race, the cached
    /// value stays ahead of the (stale) chain nonce.
    pub fn next(
        &self,
        network: impl Into<Network>,
        account_id: AccountId,
        public_key: PublicKey,
        blockchain_nonce: u64,
    ) -> u64 {
        let mut nonces = self.nonces.lock().unwrap();
        let nonce = nonces
            .entry((network.into(), account_id, public_key))
            .and_modify(|n| *n = (*n).max(blockchain_nonce))
            .or_insert(blockchain_nonce);
        *nonce += 1;
        *nonce
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nonce_manager_caching() {
        let manager = NonceManager::new();

        // First call: blockchain nonce is 10, should return 11
        let nonce1 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce1, 11);

        // Second call: blockchain nonce still 10 (stale), cached is 11, should return 12
        let nonce2 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce2, 12);

        // Third call
        let nonce3 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce3, 13);
    }

    #[test]
    fn test_chain_catches_up() {
        let manager = NonceManager::new();

        let nonce1 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce1, 11);

        // Chain advanced past our cache (e.g. another client sent txs)
        let nonce2 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            15,
        );
        assert_eq!(nonce2, 16); // max(11, 15) + 1
    }

    #[test]
    fn test_different_keys() {
        let manager = NonceManager::new();

        let nonce_alice = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce_alice, 11);

        let nonce_bob = manager.next(
            "testnet",
            "bob.testnet".parse().unwrap(),
            "ed25519:22skMptHjFWNyuEWY22ftn2AbLPSYpmYwGJRGwpNHbTV"
                .parse()
                .unwrap(),
            20,
        );
        assert_eq!(nonce_bob, 21);

        // Alice's nonce should still increment from cache
        let nonce_alice2 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce_alice2, 12);
    }

    #[test]
    fn test_different_networks() {
        let manager = NonceManager::new();

        let nonce_testnet = manager.next(
            "testnet",
            "alice.near".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce_testnet, 11);

        // Same account+key on mainnet should be separate
        let nonce_mainnet = manager.next(
            "mainnet",
            "alice.near".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            50,
        );
        assert_eq!(nonce_mainnet, 51);

        // Testnet should still increment from its own cache
        let nonce_testnet2 = manager.next(
            "testnet",
            "alice.near".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce_testnet2, 12);
    }

    #[test]
    fn test_ak_nonce_from_error() {
        let manager = NonceManager::new();

        // Simulate: sent tx with nonce 11, got InvalidNonce with ak_nonce=100
        let nonce1 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            10,
        );
        assert_eq!(nonce1, 11);

        // Retry with ak_nonce from error (higher than cache)
        let nonce2 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            100,
        );
        assert_eq!(nonce2, 101);

        // Subsequent call, chain still stale
        let nonce3 = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            100,
        );
        assert_eq!(nonce3, 102);
    }

    #[test]
    fn test_lower_ak_nonce_uses_cache() {
        let manager = NonceManager::new();

        // Build up cache
        assert_eq!(
            manager.next(
                "testnet",
                "alice.testnet".parse().unwrap(),
                "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                    .parse()
                    .unwrap(),
                100,
            ),
            101
        );
        assert_eq!(
            manager.next(
                "testnet",
                "alice.testnet".parse().unwrap(),
                "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                    .parse()
                    .unwrap(),
                100,
            ),
            102
        );
        assert_eq!(
            manager.next(
                "testnet",
                "alice.testnet".parse().unwrap(),
                "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                    .parse()
                    .unwrap(),
                100,
            ),
            103
        );

        // Lower chain nonce — cache should win
        let nonce = manager.next(
            "testnet",
            "alice.testnet".parse().unwrap(),
            "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
                .parse()
                .unwrap(),
            100,
        );
        assert_eq!(nonce, 104); // max(103, 100) + 1
    }
}
