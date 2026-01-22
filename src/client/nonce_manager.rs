//! Nonce manager for concurrent transaction handling.
//!
//! Prevents nonce collisions when sending multiple transactions in parallel
//! by caching nonces in memory and incrementing them atomically.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

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
    pub fn invalidate(&self, account_id: &str, public_key: &str) {
        let key = format!("{}:{}", account_id, public_key);
        let mut nonces = self.nonces.lock().unwrap();
        nonces.remove(&key);
    }

    /// Clear all cached nonces.
    #[allow(dead_code)]
    pub fn clear(&self) {
        let mut nonces = self.nonces.lock().unwrap();
        nonces.clear();
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
}
