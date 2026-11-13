//! near-sdk's JSON wrappers for 64- and 128-bit integers and byte vectors,
//! re-exported from `near-sdk-core`.
//!
//! JSON numbers lose precision past 2^53, so NEAR contracts take and return
//! `u64`/`u128` amounts as decimal strings and bytes as base64 strings.
//! near-sdk's contracts use `near_sdk::json_types::U128` and friends for
//! that. These are those same types, not copies: near-sdk 5.30 re-exports
//! them from `near-sdk-core`, and so does this module. A contract and a
//! client share one `U128`, so a struct with a `U128`, `Vec<U128>` or
//! `Option<U128>` field passes between near-sdk code and near-kit code with
//! no conversion, and a contract method's signature can be copied as is. A
//! crate shared by a contract and a client can name them through either
//! `near_sdk_core::json_types` or `near_kit_primitives::json_types`.
//!
//! | Type | Holds | JSON | Borsh |
//! |------|-------|------|-------|
//! | [`U64`], [`I64`] | `u64`, `i64` | `"18446744073709551615"` | the integer |
//! | [`U128`], [`I128`] | `u128`, `i128` | `"-42"` | the integer |
//! | [`Base64VecU8`] | `Vec<u8>` | `"AQID"` (standard base64, padded) | the vector |
//!
//! Each is a tuple struct with a public field (`U128(5).0 == 5`) and
//! converts to and from its inner type with `From`. Deserializing accepts a
//! JSON string that parses as the integer (so `"+5"` but not `5` or
//! `" 5"`), or standard base64 with padding.
//!
//! The traits are near-sdk's: the integers are `Copy`, `Default`, `Eq` and
//! `Ord`, and `Base64VecU8` is `Eq` and `AsRef<[u8]>`. None of them is
//! `Hash`, and `Base64VecU8` has no `Default` or `Ord`, so use the inner
//! value (`amount.0`) as a `HashMap` key or to sort bytes.
//!
//! In one build with near-sdk, that means near-sdk 5.30.x, which uses the
//! same `near-sdk-core` 4.4. near-sdk 5.27 to 5.29 pin an older
//! `near-sdk-core`, so Cargo can't resolve them together with near-kit, and
//! near-sdk 5.26 and older have `json_types` of their own, which are
//! different types.
//!
//! ```rust
//! use near_kit_primitives::json_types::{Base64VecU8, U128};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct Transfer {
//!     amount: U128,
//!     memo: Base64VecU8,
//! }
//!
//! let transfer = Transfer { amount: U128(u128::MAX), memo: Base64VecU8(vec![1, 2, 3]) };
//! let json = serde_json::to_string(&transfer)?;
//! assert_eq!(json, r#"{"amount":"340282366920938463463374607431768211455","memo":"AQID"}"#);
//!
//! let back: Transfer = serde_json::from_str(&json)?;
//! assert_eq!(u128::from(back.amount), u128::MAX);
//! // A JSON number is rejected, as in near-sdk.
//! assert!(serde_json::from_str::<U128>("10").is_err());
//! # Ok::<(), serde_json::Error>(())
//! ```

#[doc(inline)]
pub use near_sdk_core::json_types::{Base64VecU8, I64, I128, U64, U128};

#[cfg(test)]
mod tests {
    use super::*;

    fn json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap()
    }

    fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// The traits the module docs promise. The public-api snapshot lists a
    /// re-export without its impls, so this is what notices one going away.
    #[test]
    fn types_keep_their_traits() {
        fn integer<
            T: std::fmt::Debug
                + Copy
                + Default
                + Eq
                + Ord
                + serde::Serialize
                + serde::de::DeserializeOwned
                + borsh::BorshSerialize
                + borsh::BorshDeserialize
                + Send
                + Sync
                + Unpin
                + 'static,
        >() {
        }
        integer::<U64>();
        integer::<I64>();
        integer::<U128>();
        integer::<I128>();

        fn bytes<
            T: std::fmt::Debug
                + Clone
                + Eq
                + AsRef<[u8]>
                + From<Vec<u8>>
                + Into<Vec<u8>>
                + serde::Serialize
                + serde::de::DeserializeOwned
                + borsh::BorshSerialize
                + borsh::BorshDeserialize
                + Send
                + Sync
                + Unpin
                + 'static,
        >() {
        }
        bytes::<Base64VecU8>();
    }

    #[test]
    fn integers_are_decimal_strings_in_json() {
        assert_eq!(json(&U64(0)), r#""0""#);
        assert_eq!(json(&U64(u64::MAX)), r#""18446744073709551615""#);
        assert_eq!(json(&I64(i64::MIN)), r#""-9223372036854775808""#);
        assert_eq!(json(&I64(i64::MAX)), r#""9223372036854775807""#);
        assert_eq!(json(&U128(0)), r#""0""#);
        assert_eq!(
            json(&U128(u128::MAX)),
            r#""340282366920938463463374607431768211455""#
        );
        assert_eq!(json(&I128(-1)), r#""-1""#);
        assert_eq!(
            json(&I128(i128::MIN)),
            r#""-170141183460469231731687303715884105728""#
        );
        assert_eq!(
            json(&I128(i128::MAX)),
            r#""170141183460469231731687303715884105727""#
        );
    }

    #[test]
    fn integers_round_trip_at_the_edges() {
        for value in [0, 1, 10u128.pow(24), u128::MAX] {
            assert_eq!(parse::<U128>(&json(&U128(value))).unwrap(), U128(value));
        }
        for value in [i128::MIN, -1, 0, 1, i128::MAX] {
            assert_eq!(parse::<I128>(&json(&I128(value))).unwrap(), I128(value));
        }
        for value in [0, u64::MAX] {
            assert_eq!(parse::<U64>(&json(&U64(value))).unwrap(), U64(value));
        }
        for value in [i64::MIN, -1, 0, i64::MAX] {
            assert_eq!(parse::<I64>(&json(&I64(value))).unwrap(), I64(value));
        }
    }

    /// A JSON string that `str::parse` reads, and nothing else.
    #[test]
    fn integers_accept_only_decimal_strings() {
        assert_eq!(parse::<U128>(r#""+7""#).unwrap(), U128(7));
        assert_eq!(parse::<U128>(r#""007""#).unwrap(), U128(7));
        assert_eq!(parse::<I128>(r#""-0""#).unwrap(), I128(0));

        for rejected in [
            "7",
            "null",
            r#""""#,
            r#"" 7""#,
            r#""7 ""#,
            r#""-7""#,
            r#""1e3""#,
            r#""0x10""#,
            r#""1.0""#,
            r#""340282366920938463463374607431768211456""#,
        ] {
            assert!(
                parse::<U128>(rejected).is_err(),
                "{rejected} should be rejected"
            );
        }
        assert!(parse::<U64>(r#""18446744073709551616""#).is_err());
        assert!(parse::<I64>(r#""-9223372036854775809""#).is_err());
        assert!(parse::<I128>(r#""170141183460469231731687303715884105728""#).is_err());
    }

    #[test]
    fn integers_are_the_integer_in_borsh() {
        assert_eq!(borsh::to_vec(&U128(u128::MAX)).unwrap(), [0xff; 16]);
        assert_eq!(
            borsh::to_vec(&U128(1)).unwrap(),
            borsh::to_vec(&1u128).unwrap()
        );
        assert_eq!(borsh::to_vec(&I128(-1)).unwrap(), [0xff; 16]);
        assert_eq!(borsh::to_vec(&U64(258)).unwrap(), [2, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            borsh::to_vec(&I64(i64::MIN)).unwrap(),
            borsh::to_vec(&i64::MIN).unwrap()
        );
        assert_eq!(
            borsh::from_slice::<U128>(&[0xff; 16]).unwrap(),
            U128(u128::MAX)
        );
        assert_eq!(borsh::from_slice::<I128>(&[0xff; 16]).unwrap(), I128(-1));
    }

    #[test]
    fn integers_convert_both_ways() {
        assert_eq!(u128::from(U128::from(5u128)), 5);
        assert_eq!(i128::from(I128::from(-5i128)), -5);
        assert_eq!(u64::from(U64::from(5u64)), 5);
        assert_eq!(i64::from(I64::from(-5i64)), -5);
        assert_eq!(U128::default(), U128(0));
        assert!(I128(i128::MIN) < I128(i128::MAX));
    }

    #[test]
    fn bytes_are_standard_padded_base64_in_json() {
        assert_eq!(json(&Base64VecU8(vec![])), r#""""#);
        assert_eq!(json(&Base64VecU8(vec![1])), r#""AQ==""#);
        assert_eq!(json(&Base64VecU8(vec![1, 2, 3])), r#""AQID""#);
        assert_eq!(json(&Base64VecU8(vec![0xfb, 0xff])), r#""+/8=""#);
        // near-sdk's own known answer.
        assert_eq!(
            json(&Base64VecU8(vec![100, 121, 31, 20, 0, 23, 32])),
            r#""ZHkfFAAXIA==""#
        );

        assert_eq!(parse::<Base64VecU8>(r#""""#).unwrap(), Base64VecU8(vec![]));
        assert_eq!(
            parse::<Base64VecU8>(r#""+/8=""#).unwrap(),
            Base64VecU8(vec![0xfb, 0xff])
        );
        let long = Base64VecU8(vec![123; 16000]);
        assert_eq!(parse::<Base64VecU8>(&json(&long)).unwrap(), long);

        for rejected in [
            r#""AQ""#,
            r#""-_8=""#,
            r#""A Q==""#,
            r#""AQ==AQ==""#,
            "[1]",
            "null",
        ] {
            assert!(
                parse::<Base64VecU8>(rejected).is_err(),
                "{rejected} should be rejected"
            );
        }
    }

    #[test]
    fn bytes_are_the_vector_in_borsh() {
        assert_eq!(borsh::to_vec(&Base64VecU8(vec![])).unwrap(), [0, 0, 0, 0]);
        assert_eq!(
            borsh::to_vec(&Base64VecU8(vec![7, 8])).unwrap(),
            [2, 0, 0, 0, 7, 8]
        );
        assert_eq!(
            borsh::from_slice::<Base64VecU8>(&[2, 0, 0, 0, 7, 8]).unwrap(),
            Base64VecU8(vec![7, 8])
        );
        assert_eq!(Vec::from(Base64VecU8::from(vec![9])), [9]);
        assert_eq!(Base64VecU8(vec![1, 2]).as_ref(), &[1, 2]);
    }
}
