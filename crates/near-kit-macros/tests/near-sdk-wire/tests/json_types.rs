//! `near_kit::json_types` are byte-equal to `near_sdk::json_types` in JSON
//! and Borsh, and accept and reject the same JSON.

use std::fmt::Debug;

use near_kit::json_types as kit;
use near_sdk::json_types as sdk;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Both encode `kit`/`sdk` to the same JSON and Borsh bytes.
fn same_bytes<K, S>(kit: &K, sdk: &S)
where
    K: Serialize + borsh::BorshSerialize + Debug,
    S: Serialize + borsh::BorshSerialize,
{
    assert_eq!(
        serde_json::to_string(kit).unwrap(),
        serde_json::to_string(sdk).unwrap(),
        "JSON of {kit:?}"
    );
    assert_eq!(
        borsh::to_vec(kit).unwrap(),
        borsh::to_vec(sdk).unwrap(),
        "Borsh of {kit:?}"
    );
}

/// Both accept `json` or both reject it, and an accepted value is the same.
fn same_parse<K, S, T>(json: &str, kit_inner: impl Fn(K) -> T, sdk_inner: impl Fn(S) -> T)
where
    K: DeserializeOwned,
    S: DeserializeOwned,
    T: PartialEq + Debug,
{
    let kit = serde_json::from_str::<K>(json).map(kit_inner);
    let sdk = serde_json::from_str::<S>(json).map(sdk_inner);
    match (kit, sdk) {
        (Ok(kit), Ok(sdk)) => assert_eq!(kit, sdk, "{json}"),
        (Err(_), Err(_)) => {}
        (kit, sdk) => panic!("{json}: near-kit {kit:?}, near-sdk {sdk:?}"),
    }
}

/// Integer inputs: in range, out of range, signs, padding, other notations
/// and other JSON types.
const INTEGER_INPUTS: &[&str] = &[
    r#""0""#,
    r#""1""#,
    r#""+1""#,
    r#""-1""#,
    r#""-0""#,
    r#""007""#,
    r#"" 1""#,
    r#""1 ""#,
    r#""""#,
    r#""1e3""#,
    r#""0x10""#,
    r#""1.0""#,
    r#""1_000""#,
    r#""9223372036854775807""#,
    r#""9223372036854775808""#,
    r#""-9223372036854775808""#,
    r#""-9223372036854775809""#,
    r#""18446744073709551615""#,
    r#""18446744073709551616""#,
    r#""170141183460469231731687303715884105727""#,
    r#""170141183460469231731687303715884105728""#,
    r#""-170141183460469231731687303715884105728""#,
    r#""-170141183460469231731687303715884105729""#,
    r#""340282366920938463463374607431768211455""#,
    r#""340282366920938463463374607431768211456""#,
    "1",
    "-1",
    "null",
    "true",
    "[]",
];

#[test]
fn integers_encode_the_same() {
    for value in [0, 1, 10u128.pow(24), u128::MAX] {
        same_bytes(&kit::U128(value), &sdk::U128(value));
    }
    for value in [i128::MIN, -1, 0, 1, i128::MAX] {
        same_bytes(&kit::I128(value), &sdk::I128(value));
    }
    for value in [0, 1, u64::MAX] {
        same_bytes(&kit::U64(value), &sdk::U64(value));
    }
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        same_bytes(&kit::I64(value), &sdk::I64(value));
    }
}

#[test]
fn integers_accept_the_same_json() {
    for json in INTEGER_INPUTS {
        same_parse(json, |kit::U128(v)| v, |sdk::U128(v)| v);
        same_parse(json, |kit::I128(v)| v, |sdk::I128(v)| v);
        same_parse(json, |kit::U64(v)| v, |sdk::U64(v)| v);
        same_parse(json, |kit::I64(v)| v, |sdk::I64(v)| v);
    }
}

#[test]
fn integers_decode_each_others_borsh() {
    let bytes = borsh::to_vec(&sdk::U128(u128::MAX)).unwrap();
    assert_eq!(
        borsh::from_slice::<kit::U128>(&bytes).unwrap(),
        kit::U128(u128::MAX)
    );
    let bytes = borsh::to_vec(&kit::I128(i128::MIN)).unwrap();
    assert_eq!(
        borsh::from_slice::<sdk::I128>(&bytes).unwrap(),
        sdk::I128(i128::MIN)
    );
}

#[test]
fn bytes_encode_the_same() {
    for bytes in [
        vec![],
        vec![0],
        vec![1, 2],
        vec![1, 2, 3],
        vec![0xfb, 0xff],
        vec![100, 121, 31, 20, 0, 23, 32],
        (0..=255).collect(),
        vec![123; 16000],
    ] {
        same_bytes(
            &kit::Base64VecU8(bytes.clone()),
            &sdk::Base64VecU8(bytes.clone()),
        );
        let wire = borsh::to_vec(&sdk::Base64VecU8(bytes.clone())).unwrap();
        assert_eq!(
            borsh::from_slice::<kit::Base64VecU8>(&wire).unwrap(),
            kit::Base64VecU8(bytes)
        );
    }
}

#[test]
fn bytes_accept_the_same_json() {
    for json in [
        r#""""#,
        r#""AA==""#,
        r#""AQ==""#,
        r#""AQ""#,
        r#""AQ=""#,
        r#""AQ===""#,
        r#""AQI=""#,
        r#""AQI""#,
        r#""AQID""#,
        r#""+/8=""#,
        r#""-_8=""#,
        r#""A Q==""#,
        r#""AQ==\n""#,
        r#""AQ==AQ==""#,
        r#""AR==""#,
        r#""=""#,
        r#""ZHkfFAAXIA==""#,
        "null",
        "[1]",
        "1",
    ] {
        same_parse(json, |kit::Base64VecU8(v)| v, |sdk::Base64VecU8(v)| v);
    }
}
