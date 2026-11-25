//! near-kit re-exports near-global-contracts' universal state init types.
//! These pin what near-kit adds on top: the action's raw-bytes `0u` id agrees
//! with upstream's validated one on NEP-655's portable vectors, and near-kit's
//! keys convert to the handles upstream stores.

use std::collections::BTreeSet;

use near_global_contracts as up;
use near_kit_primitives::NearToken;
use near_kit_primitives::protocol::{
    Action, RawStateInit, StateInitError, UniversalStateInit, UniversalStateInitV1,
};
use near_kit_primitives::signer::{PublicKeyHandle, SecretKey};

/// near-kit's paths name upstream's types, so a value built through either
/// crate is the same value.
#[test]
fn types_are_upstreams() {
    let ours: UniversalStateInit = UniversalStateInitV1::default()
        .with_code(up::GlobalContractId::CodeHash([0x22; 32]))
        .with_data_entry("key", "value")
        .into();
    let theirs: up::UniversalStateInit = ours.clone();
    let raw: up::RawStateInit = RawStateInit::from(&theirs);
    assert_eq!(
        raw.try_derive_account_id().unwrap().as_str(),
        "0uzvdgbyea2rd8ywx0kw3cg4vc0ez1x5fc2gyks4fdz9ae0xxvzan0"
    );
    let Action::UniversalStateInit(action) =
        Action::universal_state_init(&ours, NearToken::from_yoctonear(1))
    else {
        panic!("expected a UniversalStateInit action");
    };
    assert_eq!(action.state_init, raw.0);
    assert_eq!(action.decode_state_init().unwrap(), theirs);
}

/// near-global-contracts 0.3.2's portable NEP-655 canonicality vectors
/// (`tests/data/universal_state_init.json`): versioned payload hex, without
/// the `RawStateInit` length wrapper, and the `0u` id of each valid one.
const VALID: &[(&str, &str, &str)] = &[
    (
        "empty",
        "00000000000000000000",
        "0u1kajgpx8a97y8ap8y03pvt8kbm2p2cn9k5h17bgw1wa21j88865g",
    ),
    (
        "code by account id",
        "00010109000000636f64652e6e6561720000000000000000",
        "0uartat3agnh029e3nqsqzxtz5pd3130stv6a2eafszq4vgg56kaag",
    ),
    (
        "code by hash",
        "00010022222222222222222222222222222222222222222222222222222222222222220000000000000000",
        "0uk6fe1b5tnfxn87yhtc36czcehzd4ngf25crhj17871kax56nhxx0",
    ),
    (
        "lexicographic storage keys and opaque values",
        "0000040000000000000002000000ff00020000006161170000006e6f742061206e657374656420737461746520696e697401000000620000000001000000ff010000000000000000",
        "0uf40jrgvp9rttcztes53rc41yrsagjqy8e10ep87vat8k577c5kn0",
    ),
    (
        "all handle tags and bytes",
        "0000000000000400000000000000000000000000000000000000000000000000000000000000000000000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff0100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000030000000000000000000000000000000000000000000000000000000000000000",
        "0uje8f9e2zg9xn68xbrema9bn86d8e3b670rk9znyqk1km8gxwpczg",
    ),
    (
        "key-only known answer",
        "00000000000001000000031111111111111111111111111111111111111111111111111111111111111111",
        "0ux8te7g99f9kqzdtp9h4qnwt9aczpgayymmtbdc50w199rcw3at1g",
    ),
];

const INVALID: &[(&str, &str)] = &[
    ("empty input", ""),
    ("truncated state", "00"),
    ("unknown version", "01000000000000000000"),
    ("trailing bytes", "0000000000000000000000"),
    ("invalid option tag", "00020000000000000000"),
    ("invalid code tag", "0001020000000000000000"),
    ("invalid account id", "00010101000000210000000000000000"),
    ("truncated account id", "00010109000000636f6465"),
    (
        "unsorted storage keys",
        "0000020000000100000062000000000200000061610000000000000000",
    ),
    (
        "duplicate storage key equal values",
        "000002000000010000006101000000310100000061010000003100000000",
    ),
    (
        "duplicate storage key different values",
        "000002000000010000006101000000310100000061010000003200000000",
    ),
    (
        "unsorted same-tag handles",
        "0000000000000200000000ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff000000000000000000000000000000000000000000000000000000000000000000",
    ),
    (
        "unsorted mixed-tag handles",
        "000000000000020000000300000000000000000000000000000000000000000000000000000000000000000100000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    ),
    (
        "duplicate handles",
        "00000000000002000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    ),
    (
        "invalid full ML-DSA key tag",
        "00000000000001000000020000000000000000000000000000000000000000000000000000000000000000",
    ),
    (
        "truncated handle",
        "000000000000010000000300000000000000000000000000000000000000000000000000000000000000",
    ),
    ("truncated data length", "0000010000"),
    ("oversized declared data count", "0000ffffffff"),
    ("oversized declared key length", "000001000000ffffffff"),
];

/// Valid vectors keep their id through the typed decode, upstream's
/// validated derive and near-kit's action; invalid ones are rejected by
/// `validate_canonical` and `try_derive_account_id` with the same error. The
/// action's unvalidated id is over the bytes as given either way.
#[test]
fn canonicality_vectors() {
    for (name, hex, id) in VALID {
        let raw = RawStateInit::from(hex::decode(hex).unwrap());
        raw.validate_canonical()
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let typed = UniversalStateInit::try_from(&raw).unwrap();
        assert_eq!(typed.to_raw(), raw, "{name}");
        assert_eq!(typed.derive_account_id().as_str(), *id, "{name}");
        assert_eq!(raw.try_derive_account_id().unwrap().as_str(), *id, "{name}");
        let Action::UniversalStateInit(action) =
            Action::universal_state_init(raw, NearToken::from_yoctonear(0))
        else {
            panic!("expected a UniversalStateInit action");
        };
        assert_eq!(action.derive_account_id().as_str(), *id, "{name}");
        assert_eq!(action.decode_state_init().unwrap(), typed, "{name}");
    }
    for (name, hex) in INVALID {
        let raw = RawStateInit::from(hex::decode(hex).unwrap());
        let error = raw.validate_canonical().unwrap_err();
        assert_eq!(Err(error), raw.try_derive_account_id(), "{name}");
        // The action's lenient decode accepts exactly what is merely
        // non-canonical.
        let Action::UniversalStateInit(action) =
            Action::universal_state_init(raw, NearToken::from_yoctonear(0))
        else {
            panic!("expected a UniversalStateInit action");
        };
        assert_eq!(
            action.decode_state_init().is_ok(),
            error == StateInitError::NonCanonical,
            "{name}"
        );
    }
}

/// A near-kit ML-DSA-65 key goes in as upstream's hash handle, whether passed
/// as a key or as a full near-kit handle, and the stored set has it once.
#[test]
fn ml_dsa_handles_match_upstream() {
    let pk = SecretKey::generate_ml_dsa65().public_key();
    let key: &[u8; 1952] = pk.as_bytes().try_into().unwrap();
    let theirs = up::PublicKeyHandle::from_ml_dsa_65_public_key(key);
    let ours = pk.to_ml_dsa65_hash().unwrap();
    assert_eq!(PublicKeyHandle::from(theirs.clone()), ours);

    let state_init: UniversalStateInit = UniversalStateInitV1::default()
        .with_access_key(PublicKeyHandle::Full(pk.clone()))
        .with_access_key(pk)
        .with_access_key(ours)
        .into();
    let upstream = up::UniversalStateInit::from(
        up::UniversalStateInitV1::default().with_access_key(theirs.clone()),
    );
    assert_eq!(state_init.access_keys(), &BTreeSet::from([theirs]));
    assert_eq!(state_init.to_raw(), upstream.to_raw());
}
