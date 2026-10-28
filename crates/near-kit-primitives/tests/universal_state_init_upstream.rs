//! near-kit's typed universal state init against near-global-contracts', whose
//! `RawStateInit` and `StateInitError` near-kit re-exports: same bytes, same
//! `0u` ids, same canonicality verdicts.

use std::collections::BTreeSet;

use near_global_contracts as up;
use near_kit_primitives::NearToken;
use near_kit_primitives::protocol::{
    Action, RawStateInit, StateInitError, UniversalStateInit, UniversalStateInitV1,
};
use near_kit_primitives::signer::{PublicKey, PublicKeyHandle, SecretKey};

fn to_nk_handle(handle: &up::PublicKeyHandle) -> PublicKeyHandle {
    match handle {
        up::PublicKeyHandle::ED25519(b) => PublicKeyHandle::Full(PublicKey::Ed25519(*b)),
        up::PublicKeyHandle::SECP256K1(b) => PublicKeyHandle::Full(PublicKey::Secp256k1(*b)),
        up::PublicKeyHandle::MLDSA65Hash(b) => PublicKeyHandle::MlDsa65Hash(*b),
    }
}

fn to_nk(v: &up::UniversalStateInitV1) -> UniversalStateInit {
    UniversalStateInit::V1(UniversalStateInitV1 {
        code: v.code.clone(),
        data: v.data.clone(),
        access_keys: v.access_keys.iter().map(to_nk_handle).collect(),
    })
}

fn upstream_samples() -> Vec<up::UniversalStateInitV1> {
    let v1 = up::UniversalStateInitV1::default;
    let mut ed_high = [0; 32];
    ed_high[0] = 0xff;
    let keys = [
        up::PublicKeyHandle::MLDSA65Hash([0x00; 32]),
        up::PublicKeyHandle::SECP256K1([0x01; 64]),
        up::PublicKeyHandle::ED25519(ed_high),
        up::PublicKeyHandle::ED25519([0x00; 32]),
        up::PublicKeyHandle::MLDSA65Hash([0xff; 32]),
        up::PublicKeyHandle::SECP256K1([0x00; 64]),
    ];
    let keys_only = keys.iter().cloned().fold(v1(), |s, k| s.with_access_key(k));
    let everything = keys_only
        .clone()
        .with_code(up::GlobalContractId::AccountId(
            "0sb0d7ef4f935c6ef78e08ad03569767aaec4223a3"
                .parse()
                .unwrap(),
        ))
        .with_data_entry(b"k", b"v");
    vec![
        v1(),
        v1().with_code(up::GlobalContractId::AccountId(
            "code.near".parse().unwrap(),
        )),
        v1().with_code(up::GlobalContractId::CodeHash([0x22; 32])),
        v1().with_data_entry(b"b", b"2")
            .with_data_entry(b"a", b"1")
            .with_data_entry(b"ab", b"")
            .with_data_entry(b"", b"empty-key")
            .with_data_entry([0xff, 0x00], [0x00; 3]),
        v1().with_access_key(up::PublicKeyHandle::MLDSA65Hash([0x11; 32])),
        v1().with_code(up::GlobalContractId::CodeHash([0x22; 32]))
            .with_data_entry(b"key", b"value"),
        keys_only,
        everything,
    ]
}

/// The same logical value encodes to the same bytes and `0u` id in both
/// crates, each decodes the other's bytes, and `Action::universal_state_init`
/// carries either typed value as those bytes.
#[test]
fn typed_encoding_and_ids_match_upstream() {
    for v in upstream_samples() {
        let theirs = up::UniversalStateInit::from(v.clone());
        let ours = to_nk(&v);
        let raw: RawStateInit = ours.to_raw();
        assert_eq!(raw, theirs.to_raw());
        assert_eq!(RawStateInit::from(&ours), raw);
        assert_eq!(ours.derive_account_id(), theirs.derive_account_id());
        assert_eq!(
            raw.try_derive_account_id().unwrap(),
            ours.derive_account_id()
        );
        assert_eq!(UniversalStateInit::from_raw(&raw).unwrap(), ours);
        assert_eq!(up::UniversalStateInit::try_from(&raw).unwrap(), theirs);

        let deposit = NearToken::from_yoctonear(1);
        for action in [
            Action::universal_state_init(&ours, deposit),
            Action::universal_state_init(theirs.clone(), deposit),
            Action::universal_state_init(raw.clone(), deposit),
        ] {
            let Action::UniversalStateInit(action) = action else {
                panic!("expected a UniversalStateInit action");
            };
            assert_eq!(action.state_init, raw.0);
            assert_eq!(action.derive_account_id(), ours.derive_account_id());
        }
    }
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

/// Valid vectors decode strictly and keep their id; invalid ones are rejected
/// with the same `StateInitError` by `from_raw` and upstream's
/// `validate_canonical`. The action's unvalidated id is over the bytes as
/// given either way.
#[test]
fn canonicality_vectors_match_upstream() {
    for (name, hex, id) in VALID {
        let raw = RawStateInit::from(hex::decode(hex).unwrap());
        let typed = UniversalStateInit::from_raw(&raw).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(typed.to_raw(), raw, "{name}");
        assert_eq!(typed.derive_account_id().as_str(), *id, "{name}");
        assert_eq!(raw.try_derive_account_id().unwrap().as_str(), *id, "{name}");
        let Action::UniversalStateInit(action) =
            Action::universal_state_init(raw, NearToken::from_yoctonear(0))
        else {
            panic!("expected a UniversalStateInit action");
        };
        assert_eq!(action.derive_account_id().as_str(), *id, "{name}");
    }
    for (name, hex) in INVALID {
        let raw = RawStateInit::from(hex::decode(hex).unwrap());
        let ours = UniversalStateInit::from_raw(&raw).unwrap_err();
        assert_eq!(Err(ours), raw.validate_canonical(), "{name}");
        assert_eq!(Err(ours), raw.try_derive_account_id(), "{name}");
        // The lenient decode agrees on what is merely non-canonical.
        assert_eq!(
            UniversalStateInit::from_raw_lenient(&raw).is_ok(),
            ours == StateInitError::NonCanonical,
            "{name}"
        );
    }
}

/// Handle tag 2 (a full ML-DSA-65 `PublicKey`) is never a handle.
#[test]
fn tag_two_handle_is_malformed() {
    let mut bytes = vec![0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2];
    bytes.extend_from_slice(&[0; 32]);
    let raw = RawStateInit::from(bytes);
    assert_eq!(
        UniversalStateInit::from_raw(&raw),
        Err(StateInitError::Malformed)
    );
    assert_eq!(raw.validate_canonical(), Err(StateInitError::Malformed));
}

/// Both crates hash an ML-DSA-65 key into the same handle; near-kit also
/// normalizes a full key put in through the public field.
#[test]
fn ml_dsa_handles_match_upstream() {
    let pk = SecretKey::generate_ml_dsa65().public_key();
    let key: &[u8; 1952] = pk.as_bytes().try_into().unwrap();
    let theirs = up::PublicKeyHandle::from_ml_dsa_65_public_key(key);
    let ours = pk.to_ml_dsa65_hash().unwrap();
    assert_eq!(ours, to_nk_handle(&theirs));

    let via_field = UniversalStateInit::V1(UniversalStateInitV1 {
        access_keys: BTreeSet::from([PublicKeyHandle::Full(pk), ours]),
        ..Default::default()
    });
    let upstream =
        up::UniversalStateInit::from(up::UniversalStateInitV1::default().with_access_key(theirs));
    assert_eq!(via_field.to_raw(), upstream.to_raw());
    assert_eq!(via_field.derive_account_id(), upstream.derive_account_id());
}
