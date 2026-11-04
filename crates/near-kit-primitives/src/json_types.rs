//! JSON wrappers for 64- and 128-bit integers and byte vectors, with
//! near-sdk's names and wire format.
//!
//! JSON numbers lose precision past 2^53, so NEAR contracts take and return
//! `u64`/`u128` amounts as decimal strings and bytes as base64 strings.
//! near-sdk's contracts use `near_sdk::json_types::U128` and friends for
//! that; these are the same types for the client side, so a contract
//! method's signature can be copied as is:
//!
//! | Type | Holds | JSON | Borsh |
//! |------|-------|------|-------|
//! | [`U64`], [`I64`] | `u64`, `i64` | `"18446744073709551615"` | the integer |
//! | [`U128`], [`I128`] | `u128`, `i128` | `"-42"` | the integer |
//! | [`Base64VecU8`] | `Vec<u8>` | `"AQID"` (standard base64, padded) | the vector |
//!
//! Each is a tuple struct with a public field (`U128(5).0 == 5`) and
//! converts to and from its inner type with `From`. Deserializing accepts
//! what near-sdk accepts: a JSON string that parses as the integer (so
//! `"+5"` but not `5` or `" 5"`), or standard base64 with padding.
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
//! # Ok::<(), serde_json::Error>(())
//! ```

use base64::{Engine as _, engine::general_purpose::STANDARD};
use borsh::{BorshDeserialize, BorshSerialize};

macro_rules! string_integer {
    ($(#[$doc:meta])* $name:ident, $inner:ty) => {
        $(#[$doc])*
        #[derive(
            Debug,
            Clone,
            Copy,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            BorshSerialize,
            BorshDeserialize,
        )]
        pub struct $name(pub $inner);

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = <String as serde::Deserialize>::deserialize(deserializer)?;
                text.parse().map(Self).map_err(|error| {
                    serde::de::Error::custom(format_args!("invalid number {text:?}: {error}"))
                })
            }
        }
    };
}

string_integer!(
    /// A `u64` that is a decimal string in JSON (`"10"`) and a `u64` in
    /// Borsh, like near-sdk's `json_types::U64`.
    ///
    /// ```rust
    /// use near_kit_primitives::json_types::U64;
    ///
    /// assert_eq!(serde_json::to_string(&U64(u64::MAX))?, r#""18446744073709551615""#);
    /// assert_eq!(serde_json::from_str::<U64>(r#""10""#)?, U64(10));
    /// // A JSON number is rejected, as in near-sdk.
    /// assert!(serde_json::from_str::<U64>("10").is_err());
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    U64,
    u64
);

string_integer!(
    /// An `i64` that is a decimal string in JSON (`"-10"`) and an `i64` in
    /// Borsh, like near-sdk's `json_types::I64`.
    ///
    /// ```rust
    /// use near_kit_primitives::json_types::I64;
    ///
    /// assert_eq!(serde_json::to_string(&I64(-10))?, r#""-10""#);
    /// assert_eq!(serde_json::from_str::<I64>(r#""-10""#)?, I64(-10));
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    I64,
    i64
);

string_integer!(
    /// A `u128` that is a decimal string in JSON (`"10"`) and a `u128` in
    /// Borsh, like near-sdk's `json_types::U128`. Token amounts (NEP-141
    /// `ft_balance_of`, NEP-245 `mt_transfer`) use it.
    ///
    /// ```rust
    /// use near_kit_primitives::json_types::U128;
    ///
    /// let amount = U128::from(10u128.pow(24));
    /// assert_eq!(serde_json::to_string(&amount)?, r#""1000000000000000000000000""#);
    /// assert_eq!(u128::from(amount), 10u128.pow(24));
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    U128,
    u128
);

string_integer!(
    /// An `i128` that is a decimal string in JSON (`"-10"`) and an `i128` in
    /// Borsh, like near-sdk's `json_types::I128`.
    ///
    /// ```rust
    /// use near_kit_primitives::json_types::I128;
    ///
    /// assert_eq!(serde_json::to_string(&I128(i128::MIN))?, format!("\"{}\"", i128::MIN));
    /// assert_eq!(serde_json::from_str::<I128>(r#""-10""#)?, I128(-10));
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    I128,
    i128
);

/// Bytes that are a standard base64 string in JSON (`"AQID"`) and a
/// `Vec<u8>` in Borsh, like near-sdk's `json_types::Base64VecU8`.
///
/// Serializing pads the string; deserializing requires the padding and
/// rejects the URL-safe alphabet, as near-sdk does.
///
/// ```rust
/// use near_kit_primitives::json_types::Base64VecU8;
///
/// let bytes = Base64VecU8::from(vec![1, 2, 3]);
/// assert_eq!(serde_json::to_string(&bytes)?, r#""AQID""#);
/// assert_eq!(serde_json::from_str::<Base64VecU8>(r#""AQ==""#)?, Base64VecU8(vec![1]));
/// assert_eq!(bytes.as_ref(), &[1, 2, 3]);
/// # Ok::<(), serde_json::Error>(())
/// ```
#[derive(
    Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, BorshSerialize, BorshDeserialize,
)]
pub struct Base64VecU8(pub Vec<u8>);

impl From<Vec<u8>> for Base64VecU8 {
    fn from(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }
}

impl From<Base64VecU8> for Vec<u8> {
    fn from(bytes: Base64VecU8) -> Self {
        bytes.0
    }
}

impl AsRef<[u8]> for Base64VecU8 {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl serde::Serialize for Base64VecU8 {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(&self.0))
    }
}

impl<'de> serde::Deserialize<'de> for Base64VecU8 {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = <String as serde::Deserialize>::deserialize(deserializer)?;
        STANDARD
            .decode(&text)
            .map(Self)
            .map_err(|error| serde::de::Error::custom(format_args!("invalid base64: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json<T: serde::Serialize>(value: &T) -> String {
        serde_json::to_string(value).unwrap()
    }

    fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, serde_json::Error> {
        serde_json::from_str(text)
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

    /// The inputs near-sdk accepts: a JSON string that `str::parse` reads.
    #[test]
    fn integers_accept_what_near_sdk_accepts() {
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
    fn integer_errors_name_the_input() {
        let error = parse::<U128>(r#""ten""#).unwrap_err().to_string();
        assert!(error.starts_with(r#"invalid number "ten": "#), "{error}");
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
    }
}
