//! Nonce manager for concurrent transaction handling.
//!
//! Prevents nonce collisions when sending multiple transactions in parallel
//! by caching nonces in memory and incrementing them atomically.

use std::{collections::HashMap, sync::Mutex};

use near_account_id::AccountId;

use crate::PublicKey;

pub type Network = String;

/// Manages nonces for concurrent transactions.
///
/// Prevents nonce collisions when sending multiple transactions in parallel
/// by caching nonces in memory and incrementing them atomically.
pub struct NonceManager {
    /// Cached nonces, value = last nonce used on this key
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
    /// Fetches from blockchain on first call, then increments atomically.
    /// Nonces are cached per `(network, account_id, public_key)` tuple so the
    /// same signer used against different RPC endpoints won't share state.
    ///
    /// # Arguments
    ///
    /// * `network` - Network identifier (typically the RPC URL) used to scope the cache
    /// * `account_id` - Account ID to get nonce for
    /// * `public_key` - Public key string (e.g., "ed25519:...")
    /// * `fetch_from_blockchain` - Callback to fetch current nonce from blockchain
    ///
    /// # Returns
    ///
    /// Next nonce to use for transaction
    pub fn next(
        &self,
        network: Network,
        account_id: AccountId,
        public_key: PublicKey,
        blockchain_nonce: u64,
    ) -> u64 {
        let mut nonces = self.nonces.lock().unwrap();
        let nonce = nonces
            .entry((network, account_id, public_key))
            .and_modify(|nonce| {
                *nonce = (*nonce).max(blockchain_nonce);
            })
            .or_insert(blockchain_nonce);
        *nonce += 1;
        *nonce
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_nonce_manager_caching() {
        let manager = NonceManager::new();

        // First call should fetch from blockchain
        let nonce1 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                Ok(10)
            })
            .await
            .unwrap();
        assert_eq!(nonce1, 11); // blockchain nonce + 1

        // Second call should use cached value
        let nonce2 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                panic!("Should not fetch again!")
            })
            .await
            .unwrap();
        assert_eq!(nonce2, 12); // incremented

        // Third call
        let nonce3 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                panic!("Should not fetch again!")
            })
            .await
            .unwrap();
        assert_eq!(nonce3, 13);
    }

    #[tokio::test]
    async fn test_nonce_manager_invalidate() {
        let manager = NonceManager::new();

        // Fetch initial nonce
        let nonce1 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                Ok(10)
            })
            .await
            .unwrap();
        assert_eq!(nonce1, 11);

        // Invalidate
        manager.invalidate("testnet", "alice.testnet", "ed25519:abc");

        // Next call should fetch again
        let nonce2 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                Ok(15)
            })
            .await
            .unwrap();
        assert_eq!(nonce2, 16); // Fresh fetch
    }

    #[tokio::test]
    async fn test_nonce_manager_different_keys() {
        let manager = NonceManager::new();

        let nonce_alice = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                Ok(10)
            })
            .await
            .unwrap();
        assert_eq!(nonce_alice, 11);

        let nonce_bob = manager
            .next("testnet", "bob.testnet", "ed25519:xyz", || async { Ok(20) })
            .await
            .unwrap();
        assert_eq!(nonce_bob, 21);

        // Alice's nonce should still increment
        let nonce_alice2 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                panic!("Should not fetch!")
            })
            .await
            .unwrap();
        assert_eq!(nonce_alice2, 12);
    }

    #[tokio::test]
    async fn test_nonce_manager_different_networks() {
        let manager = NonceManager::new();

        // Same account+key on testnet
        let nonce_testnet = manager
            .next("testnet", "alice.near", "ed25519:abc", || async { Ok(10) })
            .await
            .unwrap();
        assert_eq!(nonce_testnet, 11);

        // Same account+key on mainnet should fetch separately
        let nonce_mainnet = manager
            .next("mainnet", "alice.near", "ed25519:abc", || async { Ok(50) })
            .await
            .unwrap();
        assert_eq!(nonce_mainnet, 51);

        // Testnet should still increment from its own cache
        let nonce_testnet2 = manager
            .next("testnet", "alice.near", "ed25519:abc", || async {
                panic!("Should not fetch!")
            })
            .await
            .unwrap();
        assert_eq!(nonce_testnet2, 12);
    }

    #[tokio::test]
    async fn test_nonce_manager_update_and_get_next() {
        let manager = NonceManager::new();

        // First call - no entry exists
        let nonce1 = manager.update_and_get_next("testnet", "alice.testnet", "ed25519:abc", 100);
        assert_eq!(nonce1, 101); // current_nonce + 1

        // Second call - entry exists, should increment
        let nonce2 = manager.update_and_get_next("testnet", "alice.testnet", "ed25519:abc", 100);
        // Should use cached value (102) since it's higher than 101
        assert_eq!(nonce2, 102);

        // Third call with higher ak_nonce - should update cache
        let nonce3 = manager.update_and_get_next("testnet", "alice.testnet", "ed25519:abc", 110);
        assert_eq!(nonce3, 111); // New current_nonce + 1

        // Fourth call - should use updated cache
        let nonce4 = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                panic!("Should not fetch!")
            })
            .await
            .unwrap();
        assert_eq!(nonce4, 112); // Incremented from 112
    }

    #[tokio::test]
    async fn test_nonce_manager_update_respects_higher_cached() {
        let manager = NonceManager::new();

        // Setup: get some nonces to advance the cache
        let _ = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                Ok(100)
            })
            .await
            .unwrap(); // Returns 101, cache has 102
        let _ = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                panic!("no fetch")
            })
            .await
            .unwrap(); // Returns 102, cache has 103
        let _ = manager
            .next("testnet", "alice.testnet", "ed25519:abc", || async {
                panic!("no fetch")
            })
            .await
            .unwrap(); // Returns 103, cache has 104

        // Now update with a LOWER ak_nonce - should use cached value
        let nonce = manager.update_and_get_next("testnet", "alice.testnet", "ed25519:abc", 100);
        // Cache had 104, which is higher than 101, so use cached
        assert_eq!(nonce, 104);
    }
}
