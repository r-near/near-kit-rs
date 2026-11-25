//! Initial state of a `0u` universal account (nearcore 2.14), re-exported
//! from near-global-contracts.

use sha3::{Digest as _, Sha3_256};

use super::{AccountId, PublicKey, PublicKeyHandle};
use crate::error::StateInitError;

/// Prefix of every universal account id.
const UAID_PREFIX: &str = "0u";
/// Base32 symbols encoding the 256-bit hash (`ceil(256 / 5)`).
const UAID_DATA_SYMBOLS: usize = 52;
/// Crockford base32, lowercase (no `i`, `l`, `o`, `u`).
const CROCKFORD: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts),
/// as near-sdk does, so a state init passes between near-kit and contract code
/// as one type. Check bytes you were handed with
/// [`try_derive_account_id`](RawStateInit::try_derive_account_id) or
/// [`validate_canonical`](RawStateInit::validate_canonical), and forward them
/// unchanged. For the unvalidated id of the bytes an action carries, as the
/// chain computes it, use
/// [`UniversalStateInitAction::derive_account_id`](crate::protocol::UniversalStateInitAction::derive_account_id).
///
/// ---
///
pub use near_global_contracts::RawStateInit;

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts),
/// as near-sdk does. Create the account with
/// [`Action::universal_state_init`](crate::protocol::Action::universal_state_init),
/// sent to [`derive_account_id`](UniversalStateInit::derive_account_id).
///
/// Its `TryFrom<&RawStateInit>` is lenient: it accepts non-canonical bytes,
/// and re-encoding the result can change the account id. Never derive an id
/// from a value decoded that way; derive it from the bytes with
/// [`RawStateInit::try_derive_account_id`].
///
/// ---
///
pub use near_global_contracts::UniversalStateInit;

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts),
/// as near-sdk does. Start from `UniversalStateInitV1::default()`.
/// [`with_access_key`](UniversalStateInitV1::with_access_key) takes a
/// near-kit [`PublicKey`] or [`PublicKeyHandle`] and stores an ML-DSA-65 key
/// as the hash the chain keys it by. `access_keys` holds near-sdk's handle
/// type; convert one to near-kit's with `PublicKeyHandle::from`.
///
/// ---
///
pub use near_global_contracts::UniversalStateInitV1;

/// near-sdk's access-key handle, the element type of
/// [`UniversalStateInitV1::access_keys`].
type UpstreamHandle = near_global_contracts::PublicKeyHandle;

impl From<PublicKey> for UpstreamHandle {
    /// The handle the chain stores `key` under: the key itself for ed25519
    /// and secp256k1, its hash for ML-DSA-65.
    fn from(key: PublicKey) -> Self {
        match key {
            PublicKey::Ed25519(bytes) => Self::ED25519(bytes),
            PublicKey::Secp256k1(bytes) => Self::SECP256K1(bytes),
            key @ PublicKey::MlDsa65(_) => key
                .to_ml_dsa65_hash()
                .expect("an ML-DSA-65 key always has a hash")
                .into(),
        }
    }
}

impl From<PublicKeyHandle> for UpstreamHandle {
    /// Same handle; a full ML-DSA-65 key becomes its hash.
    fn from(handle: PublicKeyHandle) -> Self {
        match handle {
            PublicKeyHandle::Full(key) => key.into(),
            PublicKeyHandle::MlDsa65Hash(hash) => Self::MLDSA65Hash(hash),
        }
    }
}

impl From<UpstreamHandle> for PublicKeyHandle {
    fn from(handle: UpstreamHandle) -> Self {
        match handle {
            UpstreamHandle::ED25519(bytes) => Self::Full(PublicKey::Ed25519(bytes)),
            UpstreamHandle::SECP256K1(bytes) => Self::Full(PublicKey::Secp256k1(bytes)),
            UpstreamHandle::MLDSA65Hash(hash) => Self::MlDsa65Hash(hash),
        }
    }
}

/// Lenient decode of state init bytes, for the `UniversalStateInit` action.
pub(crate) fn decode_lenient(bytes: &[u8]) -> Result<UniversalStateInit, StateInitError> {
    borsh::from_slice(bytes).map_err(|_| StateInitError::Malformed)
}

/// The `0u` account id of state init bytes: SHA3-256 over them, as given,
/// whether they decode or not (nearcore's `derive_universal_account_id`).
pub(crate) fn derive_universal_account_id(bytes: &[u8]) -> AccountId {
    let hash: [u8; 32] = Sha3_256::digest(bytes).into();
    encode_universal_account_id(&hash)
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
    out.parse()
        .expect("a 0u id is always a valid account id: 54 lowercase alphanumerics")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::types::{GlobalContractId, NearToken, SecretKey, UniversalStateInitAction};

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

    /// Known-answer vectors from nearcore 2.14-release,
    /// `core/primitives/src/utils.rs::test_derive_universal_account_id`: the
    /// action's raw-bytes id agrees with the typed and validated ids.
    #[test]
    fn raw_id_matches_nearcore_vectors() {
        let key_only: UniversalStateInit = UniversalStateInitV1::default()
            .with_access_key(PublicKeyHandle::MlDsa65Hash([0x11; 32]))
            .into();
        let contract: UniversalStateInit = UniversalStateInitV1::default()
            .with_code(GlobalContractId::CodeHash([0x22; 32]))
            .with_data_entry("key", "value")
            .into();
        for (state_init, expected) in [
            (
                key_only,
                "0ux8te7g99f9kqzdtp9h4qnwt9aczpgayymmtbdc50w199rcw3at1g",
            ),
            (
                contract,
                "0uzvdgbyea2rd8ywx0kw3cg4vc0ez1x5fc2gyks4fdz9ae0xxvzan0",
            ),
        ] {
            let raw = state_init.to_raw();
            assert_eq!(derive_universal_account_id(&raw.0).as_str(), expected);
            assert_eq!(raw.try_derive_account_id().unwrap().as_str(), expected);
            assert_eq!(state_init.derive_account_id().as_str(), expected);
            assert_eq!(decode_lenient(&raw.0).unwrap(), state_init);
        }
    }

    /// An ML-DSA-65 key goes in as the hash the chain stores it by, whether
    /// passed as a `PublicKey` or as a full `PublicKeyHandle`, and comes back
    /// out as near-kit's hash handle.
    #[test]
    fn ml_dsa_key_is_stored_as_its_hash() {
        let pk = SecretKey::generate_ml_dsa65().public_key();
        let hash = pk.to_ml_dsa65_hash().unwrap();

        let via_key: UniversalStateInit = UniversalStateInitV1::default()
            .with_access_key(pk.clone())
            .into();
        let via_full: UniversalStateInit = UniversalStateInitV1::default()
            .with_access_key(PublicKeyHandle::Full(pk.clone()))
            .into();
        let both: UniversalStateInit = UniversalStateInitV1::default()
            .with_access_key(pk.clone())
            .with_access_key(hash.clone())
            .into();
        let key: &[u8; 1952] = pk.as_bytes().try_into().unwrap();
        let expected = BTreeSet::from([UpstreamHandle::from_ml_dsa_65_public_key(key)]);
        for state_init in [&via_key, &via_full, &both] {
            assert_eq!(state_init.access_keys(), &expected);
            assert_eq!(state_init.to_raw(), via_key.to_raw());
        }
        let back: Vec<PublicKeyHandle> = via_key
            .access_keys()
            .iter()
            .cloned()
            .map(PublicKeyHandle::from)
            .collect();
        assert_eq!(back, std::slice::from_ref(&hash));
        assert!(back[0].refers_to(&pk));
    }

    #[test]
    fn handles_convert_both_ways() {
        let ed = SecretKey::generate_ed25519().public_key();
        let secp = SecretKey::generate_secp256k1().public_key();
        for handle in [
            PublicKeyHandle::Full(ed),
            PublicKeyHandle::Full(secp),
            PublicKeyHandle::MlDsa65Hash([0x33; 32]),
        ] {
            let upstream = UpstreamHandle::from(handle.clone());
            assert_eq!(PublicKeyHandle::from(upstream), handle);
        }
    }

    /// The action's id is over the bytes it carries, not over their canonical
    /// re-encoding; the validated derive rejects them.
    #[test]
    fn non_canonical_bytes_keep_their_own_id() {
        // V1, no code, two data entries with keys "b" then "a".
        let unsorted = vec![
            0, 0, 2, 0, 0, 0, 1, 0, 0, 0, b'b', 0, 0, 0, 0, 1, 0, 0, 0, b'a', 0, 0, 0, 0, 0, 0, 0,
            0,
        ];
        let action = UniversalStateInitAction {
            state_init: unsorted.clone(),
            deposit: NearToken::from_yoctonear(0),
        };
        let lenient = action.decode_state_init().unwrap();
        assert_ne!(lenient.to_raw().0, unsorted);
        assert_eq!(
            action.derive_account_id(),
            derive_universal_account_id(&unsorted)
        );
        assert_ne!(action.derive_account_id(), lenient.derive_account_id());
        assert_eq!(
            RawStateInit(unsorted).try_derive_account_id(),
            Err(StateInitError::NonCanonical)
        );
    }
}
