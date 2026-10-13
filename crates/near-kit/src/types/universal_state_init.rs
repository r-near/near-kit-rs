//! Typed initial state of a `0u` universal account (nearcore 2.14).
//!
//! A [`UniversalStateInit`] fully describes a universal account: its contract
//! code (if any), initial storage, and full-access keys. The account id is
//! `0u` followed by the Crockford base32 of SHA3-256 over the state init's
//! borsh bytes, so whoever knows the state init knows the address before the
//! account exists.
//!
//! The id commits to the *exact* bytes, which is why the wire form is the raw
//! [`RawStateInit`] and the typed value is only a view of it. nearcore accepts
//! only the canonical encoding (sorted maps and sets without duplicates, one
//! struct per version): [`UniversalStateInit::from_raw`] rejects anything else,
//! so each logical value has exactly one id.
//!
//! # Example
//!
//! ```rust
//! use near_kit::protocol::{UniversalStateInit, UniversalStateInitV1};
//! use near_kit::signer::PublicKey;
//!
//! let key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
//! let state_init: UniversalStateInit = UniversalStateInitV1::new().with_access_key(key).into();
//!
//! let account_id = state_init.derive_account_id();
//! assert!(account_id.as_str().starts_with("0u"));
//! assert_eq!(account_id.as_str().len(), 54);
//!
//! // Round-trips through the canonical wire bytes.
//! let raw = state_init.to_raw();
//! assert_eq!(UniversalStateInit::from_raw(&raw)?, state_init);
//! assert_eq!(raw.derive_account_id(), account_id);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read, Write};

use borsh::{BorshDeserialize, BorshSerialize};
use sha3::{Digest as _, Sha3_256};

use super::{AccountId, GlobalContractId, PublicKey, PublicKeyHandle};
use crate::error::UniversalStateInitError;

/// Prefix of every universal account id.
const UAID_PREFIX: &str = "0u";
/// Base32 symbols encoding the 256-bit hash (`ceil(256 / 5)`).
const UAID_DATA_SYMBOLS: usize = 52;
/// Crockford base32, lowercase (no `i`, `l`, `o`, `u`).
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// Borsh tags of nearcore's `PublicKeyHandle`. Tag 2 belongs to a full
/// ML-DSA-65 `PublicKey` and is never valid in a handle.
const HANDLE_TAG_ED25519: u8 = 0;
const HANDLE_TAG_SECP256K1: u8 = 1;
const HANDLE_TAG_ML_DSA_65_HASH: u8 = 3;

/// The raw borsh bytes of a [`UniversalStateInit`], as carried by the
/// `UniversalStateInit` action.
///
/// This is the protocol's view of a state init: the `0u` account id is
/// SHA3-256 over exactly these bytes (not over a re-encoding of their decoded
/// value, and not over `borsh(RawStateInit)`, which adds a length prefix).
/// Build one from a typed value with [`UniversalStateInit::to_raw`], or wrap
/// bytes you received with `From<Vec<u8>>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, BorshSerialize, BorshDeserialize)]
pub struct RawStateInit(pub Vec<u8>);

impl RawStateInit {
    /// The raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The raw bytes, owned.
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    /// The `0u` universal account id these bytes define.
    ///
    /// Hashes the bytes as given and does not validate them: a non-canonical
    /// or malformed payload still derives an id, but the chain will never
    /// create that account. Use [`decode`](Self::decode) to check first.
    pub fn derive_account_id(&self) -> AccountId {
        let hash: [u8; 32] = Sha3_256::digest(&self.0).into();
        encode_universal_account_id(&hash)
    }

    /// Decode into the typed form, accepting only the canonical encoding.
    ///
    /// Same as [`UniversalStateInit::from_raw`].
    pub fn decode(&self) -> Result<UniversalStateInit, UniversalStateInitError> {
        UniversalStateInit::from_raw(self)
    }
}

impl From<Vec<u8>> for RawStateInit {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<&[u8]> for RawStateInit {
    fn from(bytes: &[u8]) -> Self {
        Self(bytes.to_vec())
    }
}

impl From<UniversalStateInit> for RawStateInit {
    fn from(state_init: UniversalStateInit) -> Self {
        state_init.to_raw()
    }
}

impl From<&UniversalStateInit> for RawStateInit {
    fn from(state_init: &UniversalStateInit) -> Self {
        state_init.to_raw()
    }
}

impl From<UniversalStateInitV1> for RawStateInit {
    fn from(state_init: UniversalStateInitV1) -> Self {
        UniversalStateInit::V1(state_init).to_raw()
    }
}

impl From<RawStateInit> for Vec<u8> {
    fn from(raw: RawStateInit) -> Self {
        raw.0
    }
}

/// Versioned initial state of a `0u` universal account.
///
/// Mirrors nearcore's `UniversalStateInit`. New fields or semantics arrive as
/// a new variant; the borsh discriminant is the only version marker.
///
/// Its [`BorshSerialize`] output is always the canonical encoding, so
/// `borsh::to_vec(&state_init)` equals [`to_raw`](Self::to_raw). Its
/// [`BorshDeserialize`] is lenient (it accepts unsorted or duplicate entries,
/// as plain borsh does); use [`from_raw`](Self::from_raw) to accept only what
/// the chain accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UniversalStateInit {
    /// Version 1 (borsh discriminant `0`).
    V1(UniversalStateInitV1),
}

/// Version 1 of [`UniversalStateInit`].
///
/// Which fields are set decides the kind of account: a key-only account has
/// no `code`; a contract account has `code` and optionally seeded `data`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UniversalStateInitV1 {
    /// Global contract the account runs, or `None` for a key-only account.
    pub code: Option<GlobalContractId>,
    /// Initial contract storage.
    pub data: BTreeMap<Vec<u8>, Vec<u8>>,
    /// Full-access keys.
    ///
    /// The chain stores ML-DSA-65 keys by their 32-byte hash, so a full
    /// ML-DSA-65 key inserted here as [`PublicKeyHandle::Full`] is encoded as
    /// that hash (and decodes back as [`PublicKeyHandle::MlDsa65Hash`]). Prefer
    /// [`with_access_key`](Self::with_access_key), which hashes it on insert.
    pub access_keys: BTreeSet<PublicKeyHandle>,
}

impl UniversalStateInitV1 {
    /// An empty state init: no code, no data, no keys.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the global contract the account runs.
    pub fn with_code(mut self, code: GlobalContractId) -> Self {
        self.code = Some(code);
        self
    }

    /// Add an initial storage entry.
    pub fn with_data_entry(mut self, key: impl Into<Vec<u8>>, value: impl Into<Vec<u8>>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    /// Add a full-access key.
    ///
    /// Accepts a [`PublicKey`] or a [`PublicKeyHandle`]. A full ML-DSA-65 key
    /// is stored as the hash the chain keys it by.
    pub fn with_access_key(mut self, key: impl Into<PublicKeyHandle>) -> Self {
        self.access_keys.insert(canonical_handle(key.into()));
        self
    }
}

impl From<UniversalStateInitV1> for UniversalStateInit {
    fn from(v1: UniversalStateInitV1) -> Self {
        Self::V1(v1)
    }
}

impl UniversalStateInit {
    /// The global contract the account runs, or `None` for a key-only account.
    pub fn code(&self) -> Option<&GlobalContractId> {
        match self {
            Self::V1(inner) => inner.code.as_ref(),
        }
    }

    /// Initial contract storage.
    pub fn data(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        match self {
            Self::V1(inner) => &inner.data,
        }
    }

    /// Full-access keys.
    pub fn access_keys(&self) -> &BTreeSet<PublicKeyHandle> {
        match self {
            Self::V1(inner) => &inner.access_keys,
        }
    }

    /// The version number (`1` for [`V1`](Self::V1)).
    pub fn version(&self) -> u32 {
        match self {
            Self::V1(_) => 1,
        }
    }

    /// The canonical borsh encoding, ready to put in a `UniversalStateInit`
    /// action.
    pub fn to_raw(&self) -> RawStateInit {
        RawStateInit(borsh::to_vec(self).expect("writing to a Vec cannot fail"))
    }

    /// Decode `raw`, accepting only the canonical encoding.
    ///
    /// This is what nearcore accepts from the `CanonicalUniversalStateInit`
    /// protocol feature on. Fails with
    /// [`Malformed`](UniversalStateInitError::Malformed) for bytes that do not
    /// decode (including trailing bytes), and with
    /// [`NonCanonical`](UniversalStateInitError::NonCanonical) for any
    /// encoding that differs from [`to_raw`](Self::to_raw) of the decoded
    /// value (unsorted or duplicate data keys or access keys).
    pub fn from_raw(raw: &RawStateInit) -> Result<Self, UniversalStateInitError> {
        let decoded = Self::from_raw_lenient(raw)?;
        if decoded.to_raw() != *raw {
            return Err(UniversalStateInitError::NonCanonical);
        }
        Ok(decoded)
    }

    /// Decode `raw`, accepting any well-formed borsh encoding.
    ///
    /// A non-canonical encoding of a value decodes, but hashes to a different
    /// id than the value's canonical one and is rejected by the chain. Use
    /// this only to inspect payloads, e.g. ones created before the
    /// `CanonicalUniversalStateInit` protocol feature.
    pub fn from_raw_lenient(raw: &RawStateInit) -> Result<Self, UniversalStateInitError> {
        borsh::from_slice(&raw.0).map_err(|_| UniversalStateInitError::Malformed)
    }

    /// The `0u` universal account id of this state init: the id of its
    /// canonical encoding.
    pub fn derive_account_id(&self) -> AccountId {
        self.to_raw().derive_account_id()
    }
}

impl BorshSerialize for UniversalStateInit {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        match self {
            Self::V1(inner) => {
                0u8.serialize(writer)?;
                inner.code.serialize(writer)?;
                // `BTreeMap` iterates in key order, which is the canonical
                // order (by key bytes, not by their length-prefixed encoding).
                inner.data.serialize(writer)?;
                // Normalize full ML-DSA-65 keys to their hash, then re-sort and
                // dedup, so the set is canonical however it was built.
                let keys: BTreeSet<PublicKeyHandle> = inner
                    .access_keys
                    .iter()
                    .cloned()
                    .map(canonical_handle)
                    .collect();
                write_len(keys.len(), writer)?;
                for key in &keys {
                    serialize_handle(key, writer)?;
                }
                Ok(())
            }
        }
    }
}

impl BorshDeserialize for UniversalStateInit {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        match u8::deserialize_reader(reader)? {
            0 => {
                let code = Option::<GlobalContractId>::deserialize_reader(reader)?;
                // Decoded by hand so the result does not depend on whether
                // borsh's `de_strict_order` feature is enabled somewhere in the
                // dependency graph: order is checked by `from_raw` instead.
                let data_len = u32::deserialize_reader(reader)?;
                let mut data = BTreeMap::new();
                for _ in 0..data_len {
                    let key = Vec::<u8>::deserialize_reader(reader)?;
                    let value = Vec::<u8>::deserialize_reader(reader)?;
                    data.insert(key, value);
                }
                let keys_len = u32::deserialize_reader(reader)?;
                let mut access_keys = BTreeSet::new();
                for _ in 0..keys_len {
                    access_keys.insert(deserialize_handle(reader)?);
                }
                Ok(Self::V1(UniversalStateInitV1 {
                    code,
                    data,
                    access_keys,
                }))
            }
            tag => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unknown UniversalStateInit version tag {tag}"),
            )),
        }
    }
}

/// Replace a full ML-DSA-65 key with the hash the chain stores it by, as
/// nearcore's `From<PublicKey> for PublicKeyHandle` does.
fn canonical_handle(handle: PublicKeyHandle) -> PublicKeyHandle {
    match handle {
        PublicKeyHandle::Full(key @ PublicKey::MlDsa65(_)) => key
            .to_ml_dsa65_hash()
            .expect("an ML-DSA-65 key always has a hash"),
        other => other,
    }
}

fn write_len<W: Write>(len: usize, writer: &mut W) -> io::Result<()> {
    let len = u32::try_from(len)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many access keys"))?;
    len.serialize(writer)
}

/// nearcore's `PublicKeyHandle` borsh: a one-byte tag, then the key bytes
/// (ed25519, secp256k1) or the 32-byte ML-DSA-65 hash.
fn serialize_handle<W: Write>(handle: &PublicKeyHandle, writer: &mut W) -> io::Result<()> {
    let tag = match handle {
        PublicKeyHandle::Full(PublicKey::Ed25519(_)) => HANDLE_TAG_ED25519,
        PublicKeyHandle::Full(PublicKey::Secp256k1(_)) => HANDLE_TAG_SECP256K1,
        PublicKeyHandle::MlDsa65Hash(_) => HANDLE_TAG_ML_DSA_65_HASH,
        PublicKeyHandle::Full(PublicKey::MlDsa65(_)) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a full ML-DSA-65 key is not a valid access key handle",
            ));
        }
    };
    tag.serialize(writer)?;
    writer.write_all(handle.as_bytes())
}

fn deserialize_handle<R: Read>(reader: &mut R) -> io::Result<PublicKeyHandle> {
    match u8::deserialize_reader(reader)? {
        HANDLE_TAG_ED25519 => Ok(PublicKeyHandle::Full(PublicKey::Ed25519(
            <[u8; 32]>::deserialize_reader(reader)?,
        ))),
        HANDLE_TAG_SECP256K1 => Ok(PublicKeyHandle::Full(PublicKey::Secp256k1(
            <[u8; 64]>::deserialize_reader(reader)?,
        ))),
        HANDLE_TAG_ML_DSA_65_HASH => Ok(PublicKeyHandle::MlDsa65Hash(
            <[u8; 32]>::deserialize_reader(reader)?,
        )),
        tag => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid access key handle tag {tag}"),
        )),
    }
}

/// Encode a 32-byte hash as a `0u` universal account id: the prefix, then 52
/// Crockford base32 symbols, most significant bit first. The final symbol
/// carries one hash bit and four zero padding bits.
fn encode_universal_account_id(hash: &[u8; 32]) -> AccountId {
    let mut out = String::with_capacity(UAID_PREFIX.len() + UAID_DATA_SYMBOLS);
    out.push_str(UAID_PREFIX);
    let mut acc: u32 = 0;
    let mut nbits: u32 = 0;
    for &byte in hash {
        acc = (acc << 8) | u32::from(byte);
        nbits += 8;
        while nbits >= 5 {
            nbits -= 5;
            out.push(CROCKFORD[((acc >> nbits) & 0x1f) as usize] as char);
        }
        acc &= (1 << nbits) - 1;
    }
    out.push(CROCKFORD[((acc << (5 - nbits)) & 0x1f) as usize] as char);
    debug_assert_eq!(out.len(), UAID_PREFIX.len() + UAID_DATA_SYMBOLS);
    out.parse()
        .expect("a 0u id is always a valid account id: 54 lowercase alphanumerics")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed_handle(b: u8) -> PublicKeyHandle {
        PublicKeyHandle::Full(PublicKey::Ed25519([b; 32]))
    }

    fn mldsa_handle(b: u8) -> PublicKeyHandle {
        PublicKeyHandle::MlDsa65Hash([b; 32])
    }

    /// Known-answer vectors from nearcore 2.14-release,
    /// `core/primitives/src/utils.rs::test_derive_universal_account_id`.
    #[test]
    fn derive_account_id_matches_nearcore_vectors() {
        let key_only = UniversalStateInit::V1(UniversalStateInitV1 {
            code: None,
            data: BTreeMap::new(),
            access_keys: BTreeSet::from([mldsa_handle(0x11)]),
        });
        let mut key_only_bytes = vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 3];
        key_only_bytes.extend_from_slice(&[0x11; 32]);

        let contract = UniversalStateInit::V1(UniversalStateInitV1 {
            code: Some(GlobalContractId::CodeHash([0x22; 32])),
            data: BTreeMap::from([(b"key".to_vec(), b"value".to_vec())]),
            access_keys: BTreeSet::new(),
        });
        let mut contract_bytes = vec![0, 1, 0];
        contract_bytes.extend_from_slice(&[0x22; 32]);
        contract_bytes.extend_from_slice(&[1, 0, 0, 0, 3, 0, 0, 0]);
        contract_bytes.extend_from_slice(b"key");
        contract_bytes.extend_from_slice(&[5, 0, 0, 0]);
        contract_bytes.extend_from_slice(b"value");
        contract_bytes.extend_from_slice(&[0, 0, 0, 0]);

        for (state_init, bytes, expected) in [
            (
                &key_only,
                key_only_bytes,
                "0ux8te7g99f9kqzdtp9h4qnwt9aczpgayymmtbdc50w199rcw3at1g",
            ),
            (
                &contract,
                contract_bytes,
                "0uzvdgbyea2rd8ywx0kw3cg4vc0ez1x5fc2gyks4fdz9ae0xxvzan0",
            ),
        ] {
            let raw = state_init.to_raw();
            assert_eq!(raw.0, bytes);
            assert_eq!(raw.derive_account_id().as_str(), expected);
            assert_eq!(state_init.derive_account_id().as_str(), expected);
            assert_eq!(UniversalStateInit::from_raw(&raw).unwrap(), *state_init);
        }
    }

    /// Encoder known-answer vectors from nearcore 2.14-release,
    /// `core/primitives-core/src/universal_account_id.rs`.
    #[test]
    fn encoder_matches_nearcore_vectors() {
        let ascending: [u8; 32] = std::array::from_fn(|i| i as u8);
        let times_seven: [u8; 32] = std::array::from_fn(|i| (7 * i) as u8);
        for (hash, expected) in [
            (
                [0x00; 32],
                "0u0000000000000000000000000000000000000000000000000000",
            ),
            (
                [0xff; 32],
                "0uzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzg",
            ),
            (
                ascending,
                "0u000g40r40m30e209185gr38e1w8124gk2gahc5rr34d1p70x3rfg",
            ),
            (
                times_seven,
                "0u003gw58w4cn32e1z8s6n8pv2d5r7ezm5hj9sn8d8nyvbvh6btbcg",
            ),
        ] {
            assert_eq!(encode_universal_account_id(&hash).as_str(), expected);
        }
    }

    /// The id follows the bytes supplied, not their re-encoding.
    #[test]
    fn derive_account_id_is_over_raw_bytes() {
        let raw = v1_bytes(&[(b"b", b""), (b"a", b"")], &[]);
        let decoded = UniversalStateInit::from_raw_lenient(&raw).unwrap();
        assert_ne!(decoded.to_raw(), raw);
        assert_ne!(raw.derive_account_id(), decoded.derive_account_id());
    }

    fn contract_init() -> UniversalStateInit {
        UniversalStateInitV1::new()
            .with_code(GlobalContractId::AccountId(
                "publisher.near".parse().unwrap(),
            ))
            .with_data_entry(b"alpha".to_vec(), b"1".to_vec())
            .with_data_entry(b"beta".to_vec(), b"2".to_vec())
            .with_access_key(ed_handle(7))
            .with_access_key(mldsa_handle(9))
            .with_access_key(PublicKeyHandle::Full(PublicKey::Secp256k1([5; 64])))
            .into()
    }

    #[test]
    fn round_trip() {
        let key_only: UniversalStateInit = UniversalStateInitV1::new()
            .with_access_key(ed_handle(1))
            .into();
        for si in [contract_init(), key_only] {
            let raw = si.to_raw();
            assert_eq!(borsh::to_vec(&si).unwrap(), raw.0);
            assert_eq!(UniversalStateInit::from_raw(&raw).unwrap(), si);
            assert_eq!(raw.decode().unwrap(), si);
        }
    }

    #[test]
    fn handle_encoding_matches_nearcore() {
        let si: UniversalStateInit = UniversalStateInitV1::new()
            .with_access_key(mldsa_handle(9))
            .with_access_key(PublicKeyHandle::Full(PublicKey::Secp256k1([5; 64])))
            .with_access_key(ed_handle(7))
            .into();
        let mut expected = vec![0, 0, 0, 0, 0, 0, 3, 0, 0, 0];
        // nearcore orders handles by variant (ed25519, secp256k1, ML-DSA-65
        // hash), then by bytes.
        expected.push(0);
        expected.extend_from_slice(&[7; 32]);
        expected.push(1);
        expected.extend_from_slice(&[5; 64]);
        expected.push(3);
        expected.extend_from_slice(&[9; 32]);
        assert_eq!(si.to_raw().0, expected);
    }

    #[test]
    fn full_ml_dsa_key_is_stored_as_its_hash() {
        let sk = crate::types::SecretKey::generate_ml_dsa65();
        let pk = sk.public_key();
        let hash = pk.to_ml_dsa65_hash().unwrap();

        let via_builder: UniversalStateInit = UniversalStateInitV1::new()
            .with_access_key(pk.clone())
            .into();
        assert_eq!(via_builder.access_keys(), &BTreeSet::from([hash.clone()]));

        // Inserted as a full key through the public field, it still encodes
        // as the hash, and decodes back as one.
        let via_field = UniversalStateInit::V1(UniversalStateInitV1 {
            access_keys: BTreeSet::from([PublicKeyHandle::Full(pk)]),
            ..Default::default()
        });
        assert_eq!(via_field.to_raw(), via_builder.to_raw());
        assert_eq!(
            UniversalStateInit::from_raw(&via_field.to_raw()).unwrap(),
            via_builder
        );
    }

    #[test]
    fn accessors() {
        let c = contract_init();
        assert!(c.code().is_some());
        assert_eq!(c.data().len(), 2);
        assert_eq!(c.access_keys().len(), 3);
        assert_eq!(c.version(), 1);
    }

    /// Borsh bytes of `V1 { code: None, data, access_keys }`, written in the
    /// order given. Ported from nearcore's `universal_state_init.rs` tests.
    fn v1_bytes(data: &[(&[u8], &[u8])], access_keys: &[PublicKeyHandle]) -> RawStateInit {
        let mut bytes = vec![0u8, 0u8];
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        for (key, value) in data {
            bytes.extend_from_slice(&(key.len() as u32).to_le_bytes());
            bytes.extend_from_slice(key);
            bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
            bytes.extend_from_slice(value);
        }
        bytes.extend_from_slice(&(access_keys.len() as u32).to_le_bytes());
        for handle in access_keys {
            serialize_handle(handle, &mut bytes).unwrap();
        }
        RawStateInit(bytes)
    }

    #[test]
    fn from_raw_accepts_hand_built_canonical() {
        let raw = v1_bytes(&[(b"a", b"1"), (b"b", b"2")], &[ed_handle(1), ed_handle(2)]);
        assert_eq!(UniversalStateInit::from_raw(&raw).unwrap().to_raw(), raw);
    }

    #[test]
    fn data_is_ordered_by_key_bytes_not_by_encoding() {
        // "ab" < "b" by content, though its length prefix is larger.
        let raw = v1_bytes(&[(b"ab", b""), (b"b", b"")], &[]);
        assert!(UniversalStateInit::from_raw(&raw).is_ok());
        let raw = v1_bytes(&[(b"b", b""), (b"ab", b"")], &[]);
        assert_eq!(
            UniversalStateInit::from_raw(&raw),
            Err(UniversalStateInitError::NonCanonical)
        );
    }

    fn non_canonical_encodings() -> [RawStateInit; 4] {
        [
            v1_bytes(&[(b"b", b""), (b"a", b"")], &[]),
            v1_bytes(&[], &[ed_handle(2), ed_handle(1)]),
            v1_bytes(&[(b"a", b"1"), (b"a", b"2")], &[]),
            v1_bytes(&[], &[ed_handle(1), ed_handle(1)]),
        ]
    }

    #[test]
    fn from_raw_rejects_non_canonical() {
        for raw in non_canonical_encodings() {
            assert_eq!(
                UniversalStateInit::from_raw(&raw),
                Err(UniversalStateInitError::NonCanonical),
                "{raw:?}"
            );
            let lenient = UniversalStateInit::from_raw_lenient(&raw).unwrap();
            assert_ne!(lenient.to_raw(), raw);
        }
    }

    #[test]
    fn from_raw_rejects_malformed() {
        let mut trailing = contract_init().to_raw().0;
        trailing.push(0);
        // Tag 2 is a full ML-DSA-65 `PublicKey`, never a handle.
        let mut full_ml_dsa_tag = vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2];
        full_ml_dsa_tag.extend_from_slice(&[0; 32]);
        let unknown_version = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        for bytes in [vec![], vec![0], trailing, full_ml_dsa_tag, unknown_version] {
            let raw = RawStateInit(bytes);
            assert_eq!(
                UniversalStateInit::from_raw(&raw),
                Err(UniversalStateInitError::Malformed),
                "{raw:?}"
            );
            assert_eq!(
                UniversalStateInit::from_raw_lenient(&raw),
                Err(UniversalStateInitError::Malformed)
            );
        }
    }

    #[test]
    fn derived_id_classifies_as_universal() {
        let id = contract_init().derive_account_id();
        assert_eq!(id.as_str().len(), 54);
        assert!(id.as_str().starts_with("0u"));
        assert!(!id.as_str()[2..].contains(['i', 'l', 'o', 'u']), "{id}");
    }
}
