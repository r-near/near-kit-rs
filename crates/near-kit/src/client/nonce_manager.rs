//! Nonce manager for concurrent transaction handling.
//!
//! Prevents nonce collisions when sending multiple transactions in parallel
//! by caching nonces in memory and incrementing them atomically.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Manages nonces for concurrent transactions.
///
/// Prevents nonce collisions when sending multiple transactions in parallel
/// by caching nonces in memory and incrementing them atomically.
pub struct NonceManager {
    /// Cached nonces: key = "accountId:publicKey", value = next nonce to use
    /// We use AtomicU64 for lock-free increment after initial fetch.
    /// Value of 0 means "not initialized" (valid nonces start at 1+).
    nonces: Mutex<HashMap<String, AtomicU64>>,
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

    /// Get the next nonce for an account and public key.
    ///
    /// Fetches from blockchain on first call, then increments atomically.
    ///
    /// # Arguments
    ///
    /// * `account_id` - Account ID to get nonce for
    /// * `public_key` - Public key string (e.g., "ed25519:...")
    /// * `fetch_from_blockchain` - Callback to fetch current nonce from blockchain
    ///
    /// # Returns
    ///
    /// Next nonce to use for transaction
    pub async fn get_next_nonce<F, Fut>(
        &self,
        account_id: &str,
        public_key: &str,
        fetch_from_blockchain: F,
    ) -> Result<u64, crate::Error>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<u64, crate::Error>>,
    {
        let key = format!("{}:{}", account_id, public_key);

        // Fast path: check if we already have a cached nonce
        {
            let nonces = self.nonces.lock().unwrap();
            if let Some(atomic) = nonces.get(&key) {
                // Atomically increment and return the previous value
                return Ok(atomic.fetch_add(1, Ordering::SeqCst));
            }
        }

        // Slow path: need to fetch from blockchain
        let blockchain_nonce = fetch_from_blockchain().await?;
        let next_nonce = blockchain_nonce + 1;

        // Store the nonce (next_nonce + 1 for future calls)
        {
            let mut nonces = self.nonces.lock().unwrap();
            // Check again in case another task beat us to it
            if let Some(atomic) = nonces.get(&key) {
                // Someone else already inserted, use theirs
                return Ok(atomic.fetch_add(1, Ordering::SeqCst));
            }
            // Insert with value = next_nonce + 1 (what the NEXT caller should get)
            nonces.insert(key, AtomicU64::new(next_nonce + 1));
        }

        Ok(next_nonce)
    }

    /// Invalidate cached nonce for an account and public key.
    ///
    /// Call this when an InvalidNonceError occurs to force a fresh fetch
    /// from the blockchain on the next transaction.
    #[allow(dead_code)]
    pub fn invalidate(&self, account_id: &str, public_key: &str) {
        let key = format!("{}:{}", account_id, public_key);
        let mut nonces = self.nonces.lock().unwrap();
        nonces.remove(&key);
    }

    /// Update the cached nonce to a known value.
    ///
    /// Use this when you receive an InvalidNonce error with `ak_nonce` -
    /// instead of invalidating and refetching, directly set the nonce
    /// to avoid thundering herd on retry.
    ///
    /// # Arguments
    ///
    /// * `account_id` - Account ID
    /// * `public_key` - Public key string (e.g., "ed25519:...")
    /// * `current_nonce` - The current nonce on chain (ak_nonce from error)
    ///
    /// # Returns
    ///
    /// The next nonce to use (current_nonce + 1)
    pub fn update_and_get_next(
        &self,
        account_id: &str,
        public_key: &str,
        current_nonce: u64,
    ) -> u64 {
        let key = format!("{}:{}", account_id, public_key);
        let next_nonce = current_nonce + 1;

        let mut nonces = self.nonces.lock().unwrap();
        if let Some(atomic) = nonces.get(&key) {
            // Update to max of current cached value and new value
            // This handles case where another worker already advanced past this nonce
            let cached = atomic.load(Ordering::SeqCst);
            if next_nonce > cached {
                atomic.store(next_nonce + 1, Ordering::SeqCst);
                return next_nonce;
            } else {
                // Cached value is already higher, use it
                return atomic.fetch_add(1, Ordering::SeqCst);
            }
        }

        // No entry, create one
        nonces.insert(key, AtomicU64::new(next_nonce + 1));
        next_nonce
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
            .get_next_nonce("alice.testnet", "ed25519:abc", || async { Ok(10) })
            .await
            .unwrap();
        assert_eq!(nonce1, 11); // blockchain nonce + 1

        // Second call should use cached value
        let nonce2 = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async {
                panic!("Should not fetch again!")
            })
            .await
            .unwrap();
        assert_eq!(nonce2, 12); // incremented

        // Third call
        let nonce3 = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async {
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
            .get_next_nonce("alice.testnet", "ed25519:abc", || async { Ok(10) })
            .await
            .unwrap();
        assert_eq!(nonce1, 11);

        // Invalidate
        manager.invalidate("alice.testnet", "ed25519:abc");

        // Next call should fetch again
        let nonce2 = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async { Ok(15) })
            .await
            .unwrap();
        assert_eq!(nonce2, 16); // Fresh fetch
    }

    #[tokio::test]
    async fn test_nonce_manager_different_keys() {
        let manager = NonceManager::new();

        let nonce_alice = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async { Ok(10) })
            .await
            .unwrap();
        assert_eq!(nonce_alice, 11);

        let nonce_bob = manager
            .get_next_nonce("bob.testnet", "ed25519:xyz", || async { Ok(20) })
            .await
            .unwrap();
        assert_eq!(nonce_bob, 21);

        // Alice's nonce should still increment
        let nonce_alice2 = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async {
                panic!("Should not fetch!")
            })
            .await
            .unwrap();
        assert_eq!(nonce_alice2, 12);
    }

    #[tokio::test]
    async fn test_nonce_manager_update_and_get_next() {
        let manager = NonceManager::new();

        // First call - no entry exists
        let nonce1 = manager.update_and_get_next("alice.testnet", "ed25519:abc", 100);
        assert_eq!(nonce1, 101); // current_nonce + 1

        // Second call - entry exists, should increment
        let nonce2 = manager.update_and_get_next("alice.testnet", "ed25519:abc", 100);
        // Should use cached value (102) since it's higher than 101
        assert_eq!(nonce2, 102);

        // Third call with higher ak_nonce - should update cache
        let nonce3 = manager.update_and_get_next("alice.testnet", "ed25519:abc", 110);
        assert_eq!(nonce3, 111); // New current_nonce + 1

        // Fourth call - should use updated cache
        let nonce4 = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async {
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
            .get_next_nonce("alice.testnet", "ed25519:abc", || async { Ok(100) })
            .await
            .unwrap(); // Returns 101, cache has 102
        let _ = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async {
                panic!("no fetch")
            })
            .await
            .unwrap(); // Returns 102, cache has 103
        let _ = manager
            .get_next_nonce("alice.testnet", "ed25519:abc", || async {
                panic!("no fetch")
            })
            .await
            .unwrap(); // Returns 103, cache has 104

        // Now update with a LOWER ak_nonce - should use cached value
        let nonce = manager.update_and_get_next("alice.testnet", "ed25519:abc", 100);
        // Cache had 104, which is higher than 101, so use cached
        assert_eq!(nonce, 104);
    }
}
