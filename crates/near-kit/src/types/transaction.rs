//! Transaction types.

use borsh::{BorshDeserialize, BorshSerialize};

use super::{AccountId, Action, CryptoHash, PublicKey, SecretKey, Signature};

/// An unsigned transaction.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Transaction {
    /// The account that signs and pays for the transaction.
    pub signer_id: AccountId,
    /// The public key of the signer.
    pub public_key: PublicKey,
    /// Nonce for replay protection (must be greater than previous nonce).
    pub nonce: u64,
    /// The account that receives the transaction.
    pub receiver_id: AccountId,
    /// A recent block hash for transaction validity.
    pub block_hash: CryptoHash,
    /// The actions to execute.
    pub actions: Vec<Action>,
}

impl Transaction {
    /// Create a new transaction.
    pub fn new(
        signer_id: AccountId,
        public_key: PublicKey,
        nonce: u64,
        receiver_id: AccountId,
        block_hash: CryptoHash,
        actions: Vec<Action>,
    ) -> Self {
        Self {
            signer_id,
            public_key,
            nonce,
            receiver_id,
            block_hash,
            actions,
        }
    }

    /// Get the hash of this transaction (for signing).
    pub fn get_hash(&self) -> CryptoHash {
        let bytes = borsh::to_vec(self).expect("transaction serialization should never fail");
        CryptoHash::hash(&bytes)
    }

    /// Get the raw bytes of this transaction (for signing).
    pub fn get_hash_and_size(&self) -> (CryptoHash, usize) {
        let bytes = borsh::to_vec(self).expect("transaction serialization should never fail");
        (CryptoHash::hash(&bytes), bytes.len())
    }

    /// Sign this transaction with a secret key.
    pub fn sign(self, signer: &SecretKey) -> SignedTransaction {
        let hash = self.get_hash();
        let signature = signer.sign(hash.as_bytes());
        SignedTransaction {
            transaction: self,
            signature,
        }
    }
}

/// A signed transaction ready to be sent.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedTransaction {
    /// The unsigned transaction.
    pub transaction: Transaction,
    /// The signature.
    pub signature: Signature,
}

impl SignedTransaction {
    /// Get the hash of the signed transaction (transaction hash).
    pub fn get_hash(&self) -> CryptoHash {
        self.transaction.get_hash()
    }

    /// Serialize to bytes for RPC submission.
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("signed transaction serialization should never fail")
    }

    /// Serialize to base64 for RPC submission.
    pub fn to_base64(&self) -> String {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        STANDARD.encode(self.to_bytes())
    }

    /// Deserialize from bytes.
    ///
    /// Use this to reconstruct a signed transaction that was serialized with [`to_bytes`](Self::to_bytes).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_kit::SignedTransaction;
    /// let bytes: Vec<u8> = /* received from offline signer */;
    /// let signed_tx = SignedTransaction::from_bytes(&bytes)?;
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::error::Error> {
        borsh::from_slice(bytes).map_err(|e| {
            crate::error::Error::InvalidTransaction(format!(
                "Failed to deserialize signed transaction: {}",
                e
            ))
        })
    }

    /// Deserialize from base64.
    ///
    /// Use this to reconstruct a signed transaction that was serialized with [`to_base64`](Self::to_base64).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::SignedTransaction;
    /// let base64_str = "AgAAAGFsaWNlLnRlc3RuZXQ...";
    /// let signed_tx = SignedTransaction::from_base64(base64_str)?;
    /// # Ok::<(), near_kit::Error>(())
    /// ```
    pub fn from_base64(s: &str) -> Result<Self, crate::error::Error> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let bytes = STANDARD.decode(s).map_err(|e| {
            crate::error::Error::InvalidTransaction(format!("Invalid base64: {}", e))
        })?;
        Self::from_bytes(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_hash() {
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        let tx = Transaction::new(
            "alice.testnet".parse().unwrap(),
            public,
            1,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![],
        );

        let hash = tx.get_hash();
        assert!(!hash.is_zero());
    }

    #[test]
    fn test_sign_transaction() {
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        let tx = Transaction::new(
            "alice.testnet".parse().unwrap(),
            public,
            1,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![],
        );

        let signed = tx.sign(&secret);
        assert!(!signed.to_bytes().is_empty());
    }
}
