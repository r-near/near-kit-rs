//! `near_kit::json_types` are `near_sdk::json_types`: the same Rust types,
//! so a value from a struct written against near-sdk passes into near-kit
//! code with no conversion. They also keep near-sdk's JSON and Borsh.

use std::any::TypeId;

use near_kit::json_types as kit;
use near_kit::protocol::FunctionCallAction;
use near_sdk::json_types as sdk;
use serde::{Deserialize, Serialize};

#[test]
fn json_types_are_near_sdks_types() {
    assert_eq!(TypeId::of::<kit::U64>(), TypeId::of::<sdk::U64>());
    assert_eq!(TypeId::of::<kit::I64>(), TypeId::of::<sdk::I64>());
    assert_eq!(TypeId::of::<kit::U128>(), TypeId::of::<sdk::U128>());
    assert_eq!(TypeId::of::<kit::I128>(), TypeId::of::<sdk::I128>());
    assert_eq!(
        TypeId::of::<kit::Base64VecU8>(),
        TypeId::of::<sdk::Base64VecU8>()
    );
}

/// A type from a crate that a contract and a client both depend on,
/// written with near-sdk's `json_types`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Deposits {
    pub amounts: Vec<sdk::U128>,
    pub limit: Option<sdk::U128>,
    pub deadline: sdk::U64,
    pub memo: sdk::Base64VecU8,
}

/// The client side, written with near-kit's `json_types`.
#[near_kit::contract]
pub trait Vault {
    #[call]
    fn deposit_many(
        &mut self,
        amounts: Vec<kit::U128>,
        limit: Option<kit::U128>,
        deadline: kit::U64,
        memo: kit::Base64VecU8,
    );
}

fn total(amounts: &[kit::U128]) -> kit::U128 {
    kit::U128(amounts.iter().map(|amount| amount.0).sum())
}

#[test]
fn values_pass_between_near_sdk_and_near_kit_with_no_conversion() {
    let deposits = Deposits {
        amounts: vec![sdk::U128(10), sdk::U128(u128::MAX - 10)],
        limit: Some(sdk::U128(5)),
        deadline: sdk::U64(7),
        memo: sdk::Base64VecU8(vec![1, 2, 3]),
    };

    // near-sdk values into near-kit parameters...
    let call = Vault::deposit_many(
        deposits.amounts.clone(),
        deposits.limit,
        deposits.deadline,
        deposits.memo.clone(),
    );
    let action = FunctionCallAction::try_from(call).unwrap();
    assert_eq!(action.method_name, "deposit_many");
    assert_eq!(
        action.args,
        br#"{"amounts":["10","340282366920938463463374607431768211445"],"limit":"5","deadline":"7","memo":"AQID"}"#
    );

    // ...and near-kit values into the near-sdk struct.
    let sum: sdk::U128 = total(&deposits.amounts);
    assert_eq!(sum, sdk::U128(u128::MAX));
    let rebuilt = Deposits {
        amounts: vec![kit::U128(10), kit::U128(u128::MAX - 10)],
        limit: Some(kit::U128(5)),
        deadline: kit::U64(7),
        memo: kit::Base64VecU8(vec![1, 2, 3]),
    };
    assert_eq!(rebuilt, deposits);
}

#[test]
fn integers_are_decimal_strings_in_json_and_the_integer_in_borsh() {
    for value in [0, 1, 10u128.pow(24), u128::MAX] {
        assert_eq!(
            serde_json::to_string(&kit::U128(value)).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            borsh::to_vec(&kit::U128(value)).unwrap(),
            value.to_le_bytes()
        );
    }
    for value in [i128::MIN, -1, 0, 1, i128::MAX] {
        assert_eq!(
            serde_json::to_string(&kit::I128(value)).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            borsh::to_vec(&kit::I128(value)).unwrap(),
            value.to_le_bytes()
        );
    }
    for value in [0, 1, u64::MAX] {
        assert_eq!(
            serde_json::to_string(&kit::U64(value)).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            borsh::to_vec(&kit::U64(value)).unwrap(),
            value.to_le_bytes()
        );
    }
    for value in [i64::MIN, -1, 0, 1, i64::MAX] {
        assert_eq!(
            serde_json::to_string(&kit::I64(value)).unwrap(),
            format!("\"{value}\"")
        );
        assert_eq!(
            borsh::to_vec(&kit::I64(value)).unwrap(),
            value.to_le_bytes()
        );
    }
}

#[test]
fn integers_accept_decimal_strings_only() {
    let u128 = |json: &str| serde_json::from_str::<kit::U128>(json).ok().map(|v| v.0);
    let i64 = |json: &str| serde_json::from_str::<kit::I64>(json).ok().map(|v| v.0);

    for (json, expected) in [
        (r#""0""#, Some(0)),
        (r#""+1""#, Some(1)),
        (r#""007""#, Some(7)),
        (
            r#""340282366920938463463374607431768211455""#,
            Some(u128::MAX),
        ),
        (r#""340282366920938463463374607431768211456""#, None),
        (r#""-1""#, None),
        (r#"" 1""#, None),
        (r#""1 ""#, None),
        (r#""""#, None),
        (r#""1e3""#, None),
        (r#""0x10""#, None),
        (r#""1.0""#, None),
        (r#""1_000""#, None),
        ("1", None),
        ("null", None),
        ("true", None),
        ("[]", None),
    ] {
        assert_eq!(u128(json), expected, "{json}");
    }
    for (json, expected) in [
        (r#""-0""#, Some(0)),
        (r#""-9223372036854775808""#, Some(i64::MIN)),
        (r#""-9223372036854775809""#, None),
        (r#""9223372036854775808""#, None),
        ("-1", None),
    ] {
        assert_eq!(i64(json), expected, "{json}");
    }
}

#[test]
fn bytes_are_standard_padded_base64_in_json_and_the_vector_in_borsh() {
    for (bytes, json) in [
        (vec![], r#""""#),
        (vec![1], r#""AQ==""#),
        (vec![1, 2, 3], r#""AQID""#),
        (vec![0xfb, 0xff], r#""+/8=""#),
        (vec![100, 121, 31, 20, 0, 23, 32], r#""ZHkfFAAXIA==""#),
    ] {
        let value = kit::Base64VecU8(bytes.clone());
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        assert_eq!(
            serde_json::from_str::<kit::Base64VecU8>(json).unwrap(),
            value
        );
        let mut borsh = (bytes.len() as u32).to_le_bytes().to_vec();
        borsh.extend(&bytes);
        assert_eq!(borsh::to_vec(&value).unwrap(), borsh);
    }
    for rejected in [
        r#""AQ""#,
        r#""AQ=""#,
        r#""AQ===""#,
        r#""-_8=""#,
        r#""A Q==""#,
        r#""AQ==\n""#,
        r#""AQ==AQ==""#,
        r#""AR==""#,
        r#""=""#,
        "null",
        "[1]",
        "1",
    ] {
        assert!(
            serde_json::from_str::<kit::Base64VecU8>(rejected).is_err(),
            "{rejected}"
        );
    }
}
