//! Transaction types.
//!
//! Two on-wire transaction forms are supported, matching nearcore's
//! backward-compatible custom borsh scheme (protocol 2.13, `GasKeys` feature):
//!
//! - [`Transaction`] is the legacy **V0** form (a bare `u64` nonce). It is
//!   serialized *tag-less*, exactly as before 2.13, and remains the default for
//!   ordinary transactions. A 2.13 node still accepts it.
//! - [`TransactionV1`] adds a [`TransactionNonce`] (which can carry a gas-key
//!   nonce index) and a [`NonceMode`]. It is required only to (a) sign with a
//!   gas key or (b) opt into strict nonce validation. A V1 is serialized as
//!   `[0x01] ++ borsh(TransactionV1)`.
//!
//! [`VersionedTransaction`] wraps both and implements the discriminating borsh
//! codec; [`SignedTransactionV1`] is the signed envelope for the versioned form.
//! The existing [`SignedTransaction`] (which always carries a V0 [`Transaction`])
//! is left untouched so all existing call sites keep working.

use std::io::{Read, Write};

use borsh::{BorshDeserialize, BorshSerialize};

use super::{AccountId, Action, CryptoHash, PublicKey, SecretKey, Signature};

/// Nonce value type, matching nearcore's `Nonce` (`u64`).
pub type Nonce = u64;

/// Index into a gas key's parallel nonces, matching nearcore's `NonceIndex`
/// (`u16`). A gas key allocates up to 1024 parallel nonces.
pub type NonceIndex = u16;

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

    /// Complete this transaction with an externally-produced signature.
    ///
    /// Use this for hardware wallet, MPC, or HSM signing workflows where you
    /// sign the transaction hash externally and then reconstruct the signed transaction.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit::*;
    /// # fn example(tx: Transaction, sig_bytes: [u8; 64]) {
    /// let hash = tx.get_hash();
    /// // sign hash externally...
    /// let signature = Signature::ed25519_from_bytes(sig_bytes);
    /// let signed = tx.complete(signature);
    /// # }
    /// ```
    pub fn complete(self, signature: Signature) -> SignedTransaction {
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

// ============================================================================
// Versioned transactions (protocol 2.13 — gas keys + strict nonce)
// ============================================================================

/// Tag byte prepended to a borsh-serialized [`TransactionV1`].
///
/// V0 is written tag-less; V1 is `[V1_TAG] ++ borsh(TransactionV1)`. See
/// [`VersionedTransaction`]'s borsh impl for how the two are discriminated.
const V1_TAG: u8 = 1;

/// The nonce carried by a [`TransactionV1`] (and, in RS-3, a `DelegateActionV2`).
///
/// Mirrors nearcore's `TransactionNonce` (`core/primitives/src/transaction.rs`):
/// variant 0 is a plain nonce used by ordinary access keys; variant 1 carries a
/// `nonce_index` selecting one of a gas key's parallel nonces.
///
/// Borsh discriminants are significant and must match nearcore: `Nonce = 0`,
/// `GasKeyNonce = 1`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum TransactionNonce {
    /// Simple nonce without index, used by ordinary access keys. (discriminant = 0)
    Nonce {
        /// The nonce value.
        nonce: Nonce,
    },
    /// Nonce with an index, used by gas keys. (discriminant = 1)
    GasKeyNonce {
        /// The nonce value for the selected parallel nonce.
        nonce: Nonce,
        /// Which of the gas key's parallel nonces this advances.
        nonce_index: NonceIndex,
    },
}

impl TransactionNonce {
    /// Create a plain (non-gas-key) nonce.
    pub fn from_nonce(nonce: Nonce) -> Self {
        Self::Nonce { nonce }
    }

    /// Create a gas-key nonce selecting parallel nonce `nonce_index`.
    pub fn from_nonce_and_index(nonce: Nonce, nonce_index: NonceIndex) -> Self {
        Self::GasKeyNonce { nonce, nonce_index }
    }

    /// The nonce value, regardless of variant.
    pub fn nonce(&self) -> Nonce {
        match self {
            Self::Nonce { nonce } => *nonce,
            Self::GasKeyNonce { nonce, .. } => *nonce,
        }
    }

    /// The gas-key nonce index, or `None` for a plain nonce.
    pub fn nonce_index(&self) -> Option<NonceIndex> {
        match self {
            Self::Nonce { .. } => None,
            Self::GasKeyNonce { nonce_index, .. } => Some(*nonce_index),
        }
    }
}

/// Controls how the transaction nonce is validated against the access key nonce.
///
/// Mirrors nearcore's borsh `NonceMode`. Borsh discriminants are significant and
/// must match nearcore: `Monotonic = 0` (the default), `Strict = 1`. This is the
/// binary/borsh counterpart of the RPC-view [`crate::types::NonceMode`] (which is
/// a JSON-only type); it is re-exported from the crate root as
/// [`TransactionNonceMode`](crate::TransactionNonceMode) to avoid a name clash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum NonceMode {
    /// Any nonce strictly greater than the current access key nonce (default).
    /// (discriminant = 0)
    #[default]
    Monotonic,
    /// Nonce must be exactly `ak_nonce + 1` (sequential ordering). (discriminant = 1)
    Strict,
}

/// An unsigned **V1** transaction (protocol 2.13).
///
/// Same fields as [`Transaction`] (V0) but the nonce is a [`TransactionNonce`]
/// (so it can carry a gas-key nonce index) and it adds a [`NonceMode`]. Use this
/// only when signing with a gas key or opting into [`NonceMode::Strict`];
/// ordinary transactions should keep using V0 [`Transaction`].
///
/// Construct one directly, or via [`Transaction::into_v1`] /
/// [`Transaction::into_gas_key_v1`].
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TransactionV1 {
    /// The account that signs and pays for the transaction.
    pub signer_id: AccountId,
    /// The public key of the signer.
    pub public_key: PublicKey,
    /// Nonce for replay protection. For a gas key this also selects which of the
    /// key's parallel nonces is advanced.
    pub nonce: TransactionNonce,
    /// The account that receives the transaction.
    pub receiver_id: AccountId,
    /// A recent block hash for transaction validity.
    pub block_hash: CryptoHash,
    /// The actions to execute.
    pub actions: Vec<Action>,
    /// Controls nonce validation mode (monotonic or strict sequential).
    pub nonce_mode: NonceMode,
}

impl TransactionV1 {
    /// Get the hash of this transaction (for signing).
    ///
    /// The hash is taken over the *versioned* borsh encoding (i.e. including the
    /// `0x01` tag byte), exactly as a node hashes it.
    pub fn get_hash(&self) -> CryptoHash {
        let bytes = borsh::to_vec(&VersionedTransactionRef::V1(self))
            .expect("transaction serialization should never fail");
        CryptoHash::hash(&bytes)
    }

    /// Sign this transaction with a secret key.
    pub fn sign(self, signer: &SecretKey) -> SignedTransactionV1 {
        let hash = self.get_hash();
        let signature = signer.sign(hash.as_bytes());
        SignedTransactionV1 {
            transaction: VersionedTransaction::V1(self),
            signature,
        }
    }

    /// Complete this transaction with an externally-produced signature.
    ///
    /// Use this for hardware wallet, MPC, or HSM signing workflows where you sign
    /// the transaction hash externally and then reconstruct the signed transaction.
    pub fn complete(self, signature: Signature) -> SignedTransactionV1 {
        SignedTransactionV1 {
            transaction: VersionedTransaction::V1(self),
            signature,
        }
    }
}

impl Transaction {
    /// Upgrade this V0 transaction to a [`TransactionV1`], keeping the same
    /// (plain) nonce and the given nonce mode.
    ///
    /// Useful to opt a plain-keyed transaction into [`NonceMode::Strict`].
    pub fn into_v1(self, nonce_mode: NonceMode) -> TransactionV1 {
        TransactionV1 {
            signer_id: self.signer_id,
            public_key: self.public_key,
            nonce: TransactionNonce::from_nonce(self.nonce),
            receiver_id: self.receiver_id,
            block_hash: self.block_hash,
            actions: self.actions,
            nonce_mode,
        }
    }

    /// Upgrade this V0 transaction to a gas-key [`TransactionV1`].
    ///
    /// The V0 `nonce` field becomes the gas key's nonce for the selected
    /// `nonce_index`. Use this to sign a transaction with a gas key.
    pub fn into_gas_key_v1(self, nonce_index: NonceIndex, nonce_mode: NonceMode) -> TransactionV1 {
        TransactionV1 {
            nonce: TransactionNonce::from_nonce_and_index(self.nonce, nonce_index),
            ..self.into_v1(nonce_mode)
        }
    }
}

/// A versioned unsigned transaction: either legacy [`Transaction`] (V0) or
/// [`TransactionV1`].
///
/// # Borsh encoding (backward-compatible, custom)
///
/// This deliberately does **not** use a derived enum discriminant. nearcore
/// keeps V0 byte-identical to the pre-2.13 unversioned transaction:
///
/// - **V0** → `borsh(Transaction)` with no tag.
/// - **V1** → `[0x01] ++ borsh(TransactionV1)`.
///
/// Deserialization peeks the first two bytes. The first field of either struct
/// is an `AccountId` (a borsh `String` prefixed by a 4-byte little-endian
/// length); since account IDs are 2..=64 bytes, that length's second byte is
/// always `0`. So: if the 2nd byte is `0` it's a V0; if the 1st byte is `1` and
/// the 2nd is non-zero it's a V1. A naive 2-variant derive would tag V0 as
/// `0x00` and break compatibility — hence the hand-rolled codec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VersionedTransaction {
    /// Legacy tag-less transaction.
    V0(Transaction),
    /// Tagged 2.13 transaction (gas keys / strict nonce).
    V1(TransactionV1),
}

/// Borrowed counterpart of [`VersionedTransaction`], so a [`TransactionV1`] hash
/// can be computed without cloning. Shares the exact same borsh encoding.
enum VersionedTransactionRef<'a> {
    #[allow(dead_code)]
    V0(&'a Transaction),
    V1(&'a TransactionV1),
}

impl BorshSerialize for VersionedTransactionRef<'_> {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            Self::V0(tx) => tx.serialize(writer),
            Self::V1(tx) => {
                V1_TAG.serialize(writer)?;
                tx.serialize(writer)
            }
        }
    }
}

impl BorshSerialize for VersionedTransaction {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            Self::V0(tx) => VersionedTransactionRef::V0(tx).serialize(writer),
            Self::V1(tx) => VersionedTransactionRef::V1(tx).serialize(writer),
        }
    }
}

impl BorshDeserialize for VersionedTransaction {
    fn deserialize_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        use std::io::{Error, ErrorKind};

        // Peek the first two bytes to discriminate V0 from V1 (see type docs).
        let u1 = u8::deserialize_reader(reader)?;
        let u2 = u8::deserialize_reader(reader)?;

        if u2 == 0 {
            // V0: both bytes belong to the AccountId length prefix; put them back.
            let prefix = [u1, u2];
            let mut reader = prefix.chain(reader);
            return Ok(Self::V0(Transaction::deserialize_reader(&mut reader)?));
        }

        if u1 == V1_TAG {
            // V1: u1 was the tag, u2 is the first byte of TransactionV1.
            let prefix = [u2];
            let mut reader = prefix.chain(reader);
            return Ok(Self::V1(TransactionV1::deserialize_reader(&mut reader)?));
        }

        Err(Error::new(
            ErrorKind::InvalidData,
            format!("invalid transaction version tag: {}", u1),
        ))
    }
}

impl VersionedTransaction {
    /// The signer account for either version.
    pub fn signer_id(&self) -> &AccountId {
        match self {
            Self::V0(tx) => &tx.signer_id,
            Self::V1(tx) => &tx.signer_id,
        }
    }

    /// The receiver account for either version.
    pub fn receiver_id(&self) -> &AccountId {
        match self {
            Self::V0(tx) => &tx.receiver_id,
            Self::V1(tx) => &tx.receiver_id,
        }
    }

    /// The signer's public key for either version.
    pub fn public_key(&self) -> &PublicKey {
        match self {
            Self::V0(tx) => &tx.public_key,
            Self::V1(tx) => &tx.public_key,
        }
    }

    /// The nonce as a [`TransactionNonce`] (V0 nonces become plain nonces).
    pub fn nonce(&self) -> TransactionNonce {
        match self {
            Self::V0(tx) => TransactionNonce::from_nonce(tx.nonce),
            Self::V1(tx) => tx.nonce,
        }
    }

    /// The nonce mode (V0 is always [`NonceMode::Monotonic`]).
    pub fn nonce_mode(&self) -> NonceMode {
        match self {
            Self::V0(_) => NonceMode::Monotonic,
            Self::V1(tx) => tx.nonce_mode,
        }
    }

    /// The actions for either version.
    pub fn actions(&self) -> &[Action] {
        match self {
            Self::V0(tx) => &tx.actions,
            Self::V1(tx) => &tx.actions,
        }
    }

    /// The recent block hash for either version.
    pub fn block_hash(&self) -> &CryptoHash {
        match self {
            Self::V0(tx) => &tx.block_hash,
            Self::V1(tx) => &tx.block_hash,
        }
    }

    /// `true` if this transaction requires the 2.13 `GasKeys` protocol feature
    /// (i.e. it is a V1).
    pub fn gas_keys_required(&self) -> bool {
        matches!(self, Self::V1(_))
    }

    /// The hash of this transaction (for signing), over its versioned borsh
    /// encoding.
    pub fn get_hash(&self) -> CryptoHash {
        let bytes = borsh::to_vec(self).expect("transaction serialization should never fail");
        CryptoHash::hash(&bytes)
    }
}

/// A signed [`VersionedTransaction`] ready to be sent.
///
/// This is the V1-capable sibling of [`SignedTransaction`]. A V0
/// `SignedTransactionV1` borsh-encodes byte-identically to a [`SignedTransaction`]
/// carrying the same transaction, so a relayer/node sees no difference.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedTransactionV1 {
    /// The unsigned, versioned transaction.
    pub transaction: VersionedTransaction,
    /// The signature over [`VersionedTransaction::get_hash`].
    pub signature: Signature,
}

impl SignedTransactionV1 {
    /// The transaction hash.
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

    /// Deserialize from bytes (round-trips [`to_bytes`](Self::to_bytes)).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::error::Error> {
        borsh::from_slice(bytes).map_err(|e| {
            crate::error::Error::InvalidTransaction(format!(
                "Failed to deserialize signed transaction: {}",
                e
            ))
        })
    }

    /// Deserialize from base64 (round-trips [`to_base64`](Self::to_base64)).
    pub fn from_base64(s: &str) -> Result<Self, crate::error::Error> {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let bytes = STANDARD.decode(s).map_err(|e| {
            crate::error::Error::InvalidTransaction(format!("Invalid base64: {}", e))
        })?;
        Self::from_bytes(&bytes)
    }
}

impl From<SignedTransaction> for SignedTransactionV1 {
    /// Wrap an existing V0 [`SignedTransaction`] as a versioned signed
    /// transaction. Encodes byte-identically.
    fn from(signed: SignedTransaction) -> Self {
        Self {
            transaction: VersionedTransaction::V0(signed.transaction),
            signature: signed.signature,
        }
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

    #[test]
    fn test_complete_matches_sign() {
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();

        let tx1 = Transaction::new(
            "alice.testnet".parse().unwrap(),
            public.clone(),
            1,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![],
        );

        let tx2 = tx1.clone();

        // Sign via the normal path
        let signed_normal = tx1.sign(&secret);

        // Sign manually via complete()
        let hash = tx2.get_hash();
        let signature = secret.sign(hash.as_bytes());
        let signed_complete = tx2.complete(signature);

        assert_eq!(signed_normal.get_hash(), signed_complete.get_hash());
        assert_eq!(signed_normal.signature, signed_complete.signature);
        assert_eq!(signed_normal.to_bytes(), signed_complete.to_bytes());
    }

    // ========================================================================
    // Versioned transaction (V0/V1) tests
    // ========================================================================

    use crate::types::NearToken;

    fn sample_v0() -> Transaction {
        let public: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        Transaction::new(
            "alice.testnet".parse().unwrap(),
            public,
            7,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![Action::transfer(NearToken::from_near(1))],
        )
    }

    /// V0 must serialize *tag-less* — byte-identical to the legacy bare
    /// `Transaction`. This is the whole point of the backward-compatible scheme.
    #[test]
    fn test_versioned_v0_is_tagless_and_byte_identical() {
        let tx = sample_v0();
        let legacy_bytes = borsh::to_vec(&tx).unwrap();
        let versioned_bytes = borsh::to_vec(&VersionedTransaction::V0(tx.clone())).unwrap();
        assert_eq!(legacy_bytes, versioned_bytes, "V0 must not add a tag byte");

        // First byte is the AccountId length prefix (>0), not a 0x00 enum tag.
        assert_ne!(
            versioned_bytes[0], 0,
            "V0 must not start with a 0x00 enum tag"
        );
    }

    /// V1 must serialize as `[0x01] ++ borsh(TransactionV1)`.
    #[test]
    fn test_versioned_v1_has_tag_byte() {
        let v1 = sample_v0().into_v1(NonceMode::Monotonic);
        let bytes = borsh::to_vec(&VersionedTransaction::V1(v1.clone())).unwrap();
        assert_eq!(bytes[0], V1_TAG, "V1 must start with the 0x01 tag");
        // Tail (after tag) is exactly borsh(TransactionV1).
        assert_eq!(&bytes[1..], borsh::to_vec(&v1).unwrap().as_slice());
    }

    /// Both versions must round-trip through the discriminating codec.
    #[test]
    fn test_versioned_roundtrip_both() {
        let v0 = VersionedTransaction::V0(sample_v0());
        let back = VersionedTransaction::try_from_slice(&borsh::to_vec(&v0).unwrap()).unwrap();
        assert_eq!(v0, back);

        let v1 = VersionedTransaction::V1(sample_v0().into_gas_key_v1(3, NonceMode::Strict));
        let back = VersionedTransaction::try_from_slice(&borsh::to_vec(&v1).unwrap()).unwrap();
        assert_eq!(v1, back);
    }

    /// A legacy `SignedTransaction` and a V0 `SignedTransactionV1` carrying the
    /// same transaction must encode byte-identically (node sees no difference).
    #[test]
    fn test_signed_v0_byte_identical_to_legacy() {
        let secret = SecretKey::generate_ed25519();
        let tx = Transaction::new(
            "alice.testnet".parse().unwrap(),
            secret.public_key(),
            7,
            "bob.testnet".parse().unwrap(),
            CryptoHash::ZERO,
            vec![Action::transfer(NearToken::from_near(1))],
        );
        let legacy = tx.clone().sign(&secret);
        let versioned: SignedTransactionV1 = legacy.clone().into();

        assert_eq!(legacy.to_bytes(), versioned.to_bytes());
        assert_eq!(legacy.get_hash(), versioned.get_hash());
    }

    /// The V1 hash is taken over the *tagged* encoding, so it differs from the
    /// same-content V0 hash. (A signature is bound to a specific version.)
    #[test]
    fn test_v1_hash_differs_from_v0_hash() {
        let v0 = sample_v0();
        let v1 = v0.clone().into_v1(NonceMode::Monotonic);
        assert_ne!(v0.get_hash(), v1.get_hash());
    }

    /// Signing a V1 and completing with the detached signature agree, and the
    /// signature verifies against the V1 (tagged) hash.
    #[test]
    fn test_v1_sign_and_verify() {
        let secret = SecretKey::generate_ed25519();
        let public = secret.public_key();
        let v1 = TransactionV1 {
            signer_id: "alice.testnet".parse().unwrap(),
            public_key: public.clone(),
            nonce: TransactionNonce::from_nonce_and_index(42, 5),
            receiver_id: "bob.testnet".parse().unwrap(),
            block_hash: CryptoHash::ZERO,
            actions: vec![Action::transfer(NearToken::from_near(2))],
            nonce_mode: NonceMode::Strict,
        };

        let hash = v1.get_hash();
        let signed = v1.clone().sign(&secret);
        assert_eq!(signed.get_hash(), hash);
        assert!(signed.signature.verify(hash.as_bytes(), &public));

        // complete() with a detached signature matches sign().
        let detached = secret.sign(hash.as_bytes());
        let completed = v1.complete(detached);
        assert_eq!(signed.to_bytes(), completed.to_bytes());

        // Round-trips through bytes.
        let back = SignedTransactionV1::from_bytes(&signed.to_bytes()).unwrap();
        assert_eq!(signed, back);
    }

    /// `TransactionNonce` borsh discriminants must match nearcore: 0 = plain,
    /// 1 = gas-key.
    #[test]
    fn test_transaction_nonce_discriminants() {
        let plain = borsh::to_vec(&TransactionNonce::from_nonce(9)).unwrap();
        assert_eq!(plain[0], 0, "plain Nonce must have discriminant 0");

        let gas = borsh::to_vec(&TransactionNonce::from_nonce_and_index(9, 2)).unwrap();
        assert_eq!(gas[0], 1, "GasKeyNonce must have discriminant 1");

        assert_eq!(TransactionNonce::from_nonce(9).nonce(), 9);
        assert_eq!(TransactionNonce::from_nonce(9).nonce_index(), None);
        assert_eq!(
            TransactionNonce::from_nonce_and_index(9, 2).nonce_index(),
            Some(2)
        );
    }

    /// `NonceMode` borsh discriminants must match nearcore: 0 = Monotonic
    /// (default), 1 = Strict.
    #[test]
    fn test_nonce_mode_discriminants() {
        assert_eq!(borsh::to_vec(&NonceMode::Monotonic).unwrap(), vec![0]);
        assert_eq!(borsh::to_vec(&NonceMode::Strict).unwrap(), vec![1]);
        assert_eq!(NonceMode::default(), NonceMode::Monotonic);
    }

    /// `into_gas_key_v1` moves the V0 nonce into a `GasKeyNonce` with the index.
    #[test]
    fn test_into_gas_key_v1() {
        let v0 = sample_v0(); // nonce 7
        let v1 = v0.clone().into_gas_key_v1(4, NonceMode::Monotonic);
        assert_eq!(v1.nonce, TransactionNonce::from_nonce_and_index(7, 4));
        assert_eq!(v1.signer_id, v0.signer_id);
        assert_eq!(v1.actions, v0.actions);
    }

    /// The accessors on `VersionedTransaction` report the right values for each
    /// version, including `gas_keys_required` and `nonce_mode`.
    #[test]
    fn test_versioned_accessors() {
        let v0 = VersionedTransaction::V0(sample_v0());
        assert!(!v0.gas_keys_required());
        assert_eq!(v0.nonce(), TransactionNonce::from_nonce(7));
        assert_eq!(v0.nonce_mode(), NonceMode::Monotonic);
        assert_eq!(v0.signer_id().as_str(), "alice.testnet");

        let v1 = VersionedTransaction::V1(sample_v0().into_gas_key_v1(1, NonceMode::Strict));
        assert!(v1.gas_keys_required());
        assert_eq!(v1.nonce(), TransactionNonce::from_nonce_and_index(7, 1));
        assert_eq!(v1.nonce_mode(), NonceMode::Strict);
    }

    /// An invalid version tag (2nd byte non-zero, 1st byte not the V1 tag) must
    /// be rejected, mirroring nearcore's deserializer.
    #[test]
    fn test_versioned_invalid_tag_rejected() {
        let bytes = vec![2u8, 5, 0, 0, 0, 0, 0, 0];
        let err = VersionedTransaction::try_from_slice(&bytes).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            err.to_string()
                .contains("invalid transaction version tag: 2")
        );
    }
}
