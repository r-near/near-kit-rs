//! NEAR token amount and gas unit types — re-exported from upstream crates
//! with near-kit ergonomic extensions.

pub use near_gas::NearGas as Gas;
pub use near_token::NearToken;

use crate::error::{ParseAmountError, ParseGasError};

// ============================================================================
// Constants (used by parsing helpers)
// ============================================================================

/// One yoctoNEAR (10^-24 NEAR).
const YOCTO_PER_NEAR: u128 = 1_000_000_000_000_000_000_000_000;
/// One milliNEAR in yoctoNEAR (10^-3 NEAR = 10^21 yocto).
const YOCTO_PER_MILLINEAR: u128 = 1_000_000_000_000_000_000_000;

/// Gas per teragas.
const GAS_PER_TGAS: u64 = 1_000_000_000_000;
/// Gas per gigagas.
const GAS_PER_GGAS: u64 = 1_000_000_000;

// ============================================================================
// IntoNearToken trait
// ============================================================================

/// Trait for types that can be converted into a NearToken.
///
/// This allows methods to accept both typed NearToken values (preferred)
/// and string representations for runtime input.
///
/// # Example
///
/// ```
/// use near_kit::{IntoNearToken, NearToken};
///
/// fn example(amount: impl IntoNearToken) {
///     let token = amount.into_near_token().unwrap();
/// }
///
/// // Preferred: typed constructor
/// example(NearToken::from_near(5));
///
/// // Also works: string parsing (for runtime input)
/// example("5 NEAR");
/// ```
pub trait IntoNearToken {
    /// Convert into a NearToken.
    fn into_near_token(self) -> Result<NearToken, ParseAmountError>;
}

impl IntoNearToken for NearToken {
    fn into_near_token(self) -> Result<NearToken, ParseAmountError> {
        Ok(self)
    }
}

impl IntoNearToken for &str {
    fn into_near_token(self) -> Result<NearToken, ParseAmountError> {
        parse_near_token(self)
    }
}

impl IntoNearToken for String {
    fn into_near_token(self) -> Result<NearToken, ParseAmountError> {
        parse_near_token(&self)
    }
}

impl IntoNearToken for &String {
    fn into_near_token(self) -> Result<NearToken, ParseAmountError> {
        parse_near_token(self)
    }
}

// ============================================================================
// IntoGas trait
// ============================================================================

/// Trait for types that can be converted into Gas.
///
/// This allows methods to accept both typed Gas values (preferred)
/// and string representations for runtime input.
///
/// # Example
///
/// ```
/// use near_kit::{Gas, IntoGas};
///
/// fn example(gas: impl IntoGas) {
///     let g = gas.into_gas().unwrap();
/// }
///
/// // Preferred: typed constructor
/// example(Gas::from_tgas(30));
///
/// // Also works: string parsing (for runtime input)
/// example("30 Tgas");
/// ```
pub trait IntoGas {
    /// Convert into Gas.
    fn into_gas(self) -> Result<Gas, ParseGasError>;
}

impl IntoGas for Gas {
    fn into_gas(self) -> Result<Gas, ParseGasError> {
        Ok(self)
    }
}

impl IntoGas for &str {
    fn into_gas(self) -> Result<Gas, ParseGasError> {
        parse_gas(self)
    }
}

impl IntoGas for String {
    fn into_gas(self) -> Result<Gas, ParseGasError> {
        parse_gas(&self)
    }
}

impl IntoGas for &String {
    fn into_gas(self) -> Result<Gas, ParseGasError> {
        parse_gas(self)
    }
}

// ============================================================================
// String parsing helpers (near-kit specific formats)
// ============================================================================

/// Parse a decimal NEAR string (e.g., "1.5") into yoctoNEAR.
fn parse_near_decimal(s: &str) -> Result<NearToken, ParseAmountError> {
    let s = s.trim();

    if let Some(dot_pos) = s.find('.') {
        let integer_part = &s[..dot_pos];
        let decimal_part = &s[dot_pos + 1..];

        let integer: u128 = if integer_part.is_empty() {
            0
        } else {
            integer_part
                .parse()
                .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?
        };

        let decimal_str = if decimal_part.len() > 24 {
            &decimal_part[..24]
        } else {
            decimal_part
        };

        let decimal: u128 = if decimal_str.is_empty() {
            0
        } else {
            decimal_str
                .parse()
                .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?
        };

        let decimal_scale = 24 - decimal_str.len();
        let decimal_yocto = decimal * 10u128.pow(decimal_scale as u32);

        let total = integer
            .checked_mul(YOCTO_PER_NEAR)
            .and_then(|v| v.checked_add(decimal_yocto))
            .ok_or(ParseAmountError::Overflow)?;

        Ok(NearToken::from_yoctonear(total))
    } else {
        let near: u128 = s
            .parse()
            .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?;
        near.checked_mul(YOCTO_PER_NEAR)
            .map(NearToken::from_yoctonear)
            .ok_or(ParseAmountError::Overflow)
    }
}

/// Parse a NearToken from a near-kit format string.
///
/// Supported formats:
/// - `"5 NEAR"` or `"5 near"` — whole NEAR
/// - `"1.5 NEAR"` — decimal NEAR
/// - `"500 milliNEAR"` or `"500 mNEAR"` — milliNEAR
/// - `"1000 yocto"` or `"1000 yoctoNEAR"` — yoctoNEAR
///
/// Raw numbers are NOT accepted to prevent unit confusion.
pub fn parse_near_token(s: &str) -> Result<NearToken, ParseAmountError> {
    let s = s.trim();

    // "X NEAR" or "X near"
    if let Some(value) = s.strip_suffix(" NEAR").or_else(|| s.strip_suffix(" near")) {
        return parse_near_decimal(value.trim());
    }

    // "X milliNEAR" or "X mNEAR"
    if let Some(value) = s
        .strip_suffix(" milliNEAR")
        .or_else(|| s.strip_suffix(" mNEAR"))
    {
        let v: u128 = value
            .trim()
            .parse()
            .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?;
        return v
            .checked_mul(YOCTO_PER_MILLINEAR)
            .map(NearToken::from_yoctonear)
            .ok_or(ParseAmountError::Overflow);
    }

    // "X yocto" or "X yoctoNEAR"
    if let Some(value) = s
        .strip_suffix(" yoctoNEAR")
        .or_else(|| s.strip_suffix(" yocto"))
    {
        let v: u128 = value
            .trim()
            .parse()
            .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?;
        return Ok(NearToken::from_yoctonear(v));
    }

    // Bare number = error (ambiguous)
    if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return Err(ParseAmountError::AmbiguousAmount(s.to_string()));
    }

    Err(ParseAmountError::InvalidFormat(s.to_string()))
}

/// Parse a Gas value from a near-kit format string.
///
/// Supported formats:
/// - `"30 Tgas"` or `"30 tgas"` or `"30 TGas"` — teragas (10^12)
/// - `"5 Ggas"` or `"5 ggas"` or `"5 GGas"` — gigagas (10^9)
/// - `"1000000 gas"` — raw gas units
pub fn parse_gas(s: &str) -> Result<Gas, ParseGasError> {
    let s = s.trim();

    // "X Tgas" or "X tgas" or "X TGas"
    if let Some(value) = s
        .strip_suffix(" Tgas")
        .or_else(|| s.strip_suffix(" tgas"))
        .or_else(|| s.strip_suffix(" TGas"))
    {
        let v: u64 = value
            .trim()
            .parse()
            .map_err(|_| ParseGasError::InvalidNumber(s.to_string()))?;
        return v
            .checked_mul(GAS_PER_TGAS)
            .map(Gas::from_gas)
            .ok_or(ParseGasError::Overflow);
    }

    // "X Ggas" or "X ggas" or "X GGas"
    if let Some(value) = s
        .strip_suffix(" Ggas")
        .or_else(|| s.strip_suffix(" ggas"))
        .or_else(|| s.strip_suffix(" GGas"))
    {
        let v: u64 = value
            .trim()
            .parse()
            .map_err(|_| ParseGasError::InvalidNumber(s.to_string()))?;
        return v
            .checked_mul(GAS_PER_GGAS)
            .map(Gas::from_gas)
            .ok_or(ParseGasError::Overflow);
    }

    // "X gas"
    if let Some(value) = s.strip_suffix(" gas") {
        let v: u64 = value
            .trim()
            .parse()
            .map_err(|_| ParseGasError::InvalidNumber(s.to_string()))?;
        return Ok(Gas::from_gas(v));
    }

    Err(ParseGasError::InvalidFormat(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // NearToken parsing tests
    // ========================================================================

    #[test]
    fn test_near_token_parsing() {
        assert_eq!(
            parse_near_token("5 NEAR").unwrap().as_yoctonear(),
            5 * YOCTO_PER_NEAR
        );
        assert_eq!(
            parse_near_token("1.5 NEAR").unwrap().as_yoctonear(),
            YOCTO_PER_NEAR + YOCTO_PER_NEAR / 2
        );
        assert_eq!(
            parse_near_token("100 milliNEAR").unwrap().as_yoctonear(),
            100 * YOCTO_PER_MILLINEAR
        );
        assert_eq!(parse_near_token("1000 yocto").unwrap().as_yoctonear(), 1000);
    }

    #[test]
    fn test_near_token_ambiguous() {
        assert!(matches!(
            parse_near_token("123"),
            Err(ParseAmountError::AmbiguousAmount(_))
        ));
    }

    #[test]
    fn test_gas_parsing() {
        assert_eq!(parse_gas("30 Tgas").unwrap().as_gas(), 30 * GAS_PER_TGAS);
        assert_eq!(parse_gas("5 Ggas").unwrap().as_gas(), 5 * GAS_PER_GGAS);
        assert_eq!(parse_gas("1000 gas").unwrap().as_gas(), 1000);
    }

    // ========================================================================
    // NearToken constructor tests
    // ========================================================================

    #[test]
    fn test_near_token_constructors() {
        assert_eq!(NearToken::from_near(5).as_yoctonear(), 5 * YOCTO_PER_NEAR);
        assert_eq!(
            NearToken::from_millinear(500).as_yoctonear(),
            500 * YOCTO_PER_MILLINEAR
        );
        assert_eq!(NearToken::from_yoctonear(1000).as_yoctonear(), 1000);
    }

    #[test]
    fn test_near_token_as_near() {
        assert_eq!(NearToken::from_near(5).as_near(), 5);
        assert_eq!(NearToken::from_millinear(500).as_near(), 0); // Truncated
        assert_eq!(NearToken::from_millinear(1500).as_near(), 1); // Truncated
    }

    #[test]
    fn test_near_token_is_zero() {
        assert!(NearToken::from_yoctonear(0).is_zero());
        assert!(!NearToken::from_yoctonear(1).is_zero());
    }

    // ========================================================================
    // NearToken arithmetic tests
    // ========================================================================

    #[test]
    fn test_near_token_checked_add() {
        let a = NearToken::from_near(5);
        let b = NearToken::from_near(3);
        assert_eq!(a.checked_add(b).unwrap().as_near(), 8);

        // Overflow
        let max = NearToken::from_yoctonear(u128::MAX);
        assert!(max.checked_add(NearToken::from_yoctonear(1)).is_none());
    }

    #[test]
    fn test_near_token_checked_sub() {
        let a = NearToken::from_near(5);
        let b = NearToken::from_near(3);
        assert_eq!(a.checked_sub(b).unwrap().as_near(), 2);

        // Underflow
        assert!(b.checked_sub(a).is_none());
    }

    #[test]
    fn test_near_token_saturating_add() {
        let a = NearToken::from_near(5);
        let b = NearToken::from_near(3);
        assert_eq!(a.saturating_add(b).as_near(), 8);

        // Saturates at max
        let max = NearToken::from_yoctonear(u128::MAX);
        assert_eq!(max.saturating_add(NearToken::from_yoctonear(1)), max);
    }

    #[test]
    fn test_near_token_saturating_sub() {
        let a = NearToken::from_near(5);
        let b = NearToken::from_near(3);
        assert_eq!(a.saturating_sub(b).as_near(), 2);

        // Saturates at zero
        assert_eq!(b.saturating_sub(a), NearToken::from_yoctonear(0));
    }

    // ========================================================================
    // NearToken parsing edge cases
    // ========================================================================

    #[test]
    fn test_near_token_parse_lowercase() {
        assert_eq!(parse_near_token("5 near").unwrap().as_near(), 5);
    }

    #[test]
    fn test_near_token_parse_mnear() {
        assert_eq!(
            parse_near_token("100 mNEAR").unwrap().as_yoctonear(),
            100 * YOCTO_PER_MILLINEAR
        );
    }

    #[test]
    fn test_near_token_parse_yoctonear() {
        assert_eq!(
            parse_near_token("12345 yoctoNEAR").unwrap().as_yoctonear(),
            12345
        );
    }

    #[test]
    fn test_near_token_parse_decimal_near() {
        assert_eq!(
            parse_near_token("0.5 NEAR").unwrap().as_yoctonear(),
            YOCTO_PER_NEAR / 2
        );
        assert_eq!(
            parse_near_token(".25 NEAR").unwrap().as_yoctonear(),
            YOCTO_PER_NEAR / 4
        );
    }

    #[test]
    fn test_near_token_parse_with_whitespace() {
        assert_eq!(parse_near_token("  5 NEAR  ").unwrap().as_near(), 5);
    }

    #[test]
    fn test_near_token_parse_invalid_format() {
        assert!(matches!(
            parse_near_token("5 ETH"),
            Err(ParseAmountError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_near_token_parse_invalid_number() {
        assert!(matches!(
            parse_near_token("abc NEAR"),
            Err(ParseAmountError::InvalidNumber(_))
        ));
    }

    #[test]
    fn test_near_token_try_from_str() {
        let token = "5 NEAR".into_near_token().unwrap();
        assert_eq!(token.as_near(), 5);
    }

    // ========================================================================
    // NearToken serde tests
    // ========================================================================

    #[test]
    fn test_near_token_serde_roundtrip() {
        let amount = NearToken::from_near(5);
        let json = serde_json::to_string(&amount).unwrap();
        // Upstream serializes as string (yoctoNEAR)
        assert_eq!(json, format!("\"{}\"", amount.as_yoctonear()));

        let parsed: NearToken = serde_json::from_str(&json).unwrap();
        assert_eq!(amount, parsed);
    }

    #[test]
    fn test_near_token_borsh_roundtrip() {
        let amount = NearToken::from_near(10);
        let bytes = borsh::to_vec(&amount).unwrap();
        let parsed: NearToken = borsh::from_slice(&bytes).unwrap();
        assert_eq!(amount, parsed);
    }

    // ========================================================================
    // NearToken comparison tests
    // ========================================================================

    #[test]
    fn test_near_token_ord() {
        let small = NearToken::from_near(1);
        let large = NearToken::from_near(10);
        assert!(small < large);
        assert!(large > small);
        assert!(small <= small);
        assert!(small >= small);
    }

    #[test]
    fn test_near_token_eq() {
        let a = NearToken::from_near(5);
        let b = NearToken::from_millinear(5000);
        assert_eq!(a, b);
    }

    #[test]
    fn test_near_token_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(NearToken::from_near(1));
        set.insert(NearToken::from_near(2));
        assert!(set.contains(&NearToken::from_near(1)));
        assert!(!set.contains(&NearToken::from_near(3)));
    }

    // ========================================================================
    // Gas tests
    // ========================================================================

    #[test]
    fn test_gas_constructors() {
        assert_eq!(Gas::from_gas(1000).as_gas(), 1000);
        assert_eq!(Gas::from_tgas(30).as_gas(), 30 * GAS_PER_TGAS);
        assert_eq!(Gas::from_ggas(5).as_gas(), 5 * GAS_PER_GGAS);
    }

    #[test]
    fn test_gas_as_accessors() {
        let gas = Gas::from_tgas(30);
        assert_eq!(gas.as_tgas(), 30);
        assert_eq!(gas.as_ggas(), 30_000);
        assert_eq!(gas.as_gas(), 30 * GAS_PER_TGAS);
    }

    #[test]
    fn test_gas_is_zero() {
        assert!(Gas::from_gas(0).is_zero());
        assert!(!Gas::from_ggas(1).is_zero());
    }

    #[test]
    fn test_gas_checked_add() {
        let a = Gas::from_tgas(10);
        let b = Gas::from_tgas(20);
        assert_eq!(a.checked_add(b).unwrap().as_tgas(), 30);

        // Overflow
        let max = Gas::from_gas(u64::MAX);
        assert!(max.checked_add(Gas::from_gas(1)).is_none());
    }

    #[test]
    fn test_gas_checked_sub() {
        let a = Gas::from_tgas(30);
        let b = Gas::from_tgas(10);
        assert_eq!(a.checked_sub(b).unwrap().as_tgas(), 20);

        // Underflow
        assert!(b.checked_sub(a).is_none());
    }

    #[test]
    fn test_gas_parse_tgas_variants() {
        assert_eq!(parse_gas("30 Tgas").unwrap().as_tgas(), 30);
        assert_eq!(parse_gas("30 tgas").unwrap().as_tgas(), 30);
        assert_eq!(parse_gas("30 TGas").unwrap().as_tgas(), 30);
    }

    #[test]
    fn test_gas_parse_ggas_variants() {
        assert_eq!(parse_gas("5 Ggas").unwrap().as_ggas(), 5);
        assert_eq!(parse_gas("5 ggas").unwrap().as_ggas(), 5);
        assert_eq!(parse_gas("5 GGas").unwrap().as_ggas(), 5);
    }

    #[test]
    fn test_gas_parse_invalid_format() {
        assert!(matches!(
            parse_gas("30 teragas"),
            Err(ParseGasError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_gas_parse_invalid_number() {
        assert!(matches!(
            parse_gas("abc Tgas"),
            Err(ParseGasError::InvalidNumber(_))
        ));
    }

    #[test]
    fn test_gas_try_from_str() {
        let gas = "30 Tgas".into_gas().unwrap();
        assert_eq!(gas.as_tgas(), 30);
    }

    #[test]
    fn test_gas_serde_roundtrip() {
        let gas = Gas::from_tgas(30);
        let json = serde_json::to_string(&gas).unwrap();
        let parsed: Gas = serde_json::from_str(&json).unwrap();
        assert_eq!(gas, parsed);
    }

    #[test]
    fn test_gas_borsh_roundtrip() {
        let gas = Gas::from_tgas(30);
        let bytes = borsh::to_vec(&gas).unwrap();
        let parsed: Gas = borsh::from_slice(&bytes).unwrap();
        assert_eq!(gas, parsed);
    }

    #[test]
    fn test_gas_ord() {
        let small = Gas::from_tgas(10);
        let large = Gas::from_tgas(100);
        assert!(small < large);
    }

    // ========================================================================
    // IntoNearToken tests
    // ========================================================================

    #[test]
    fn test_into_near_token_from_near_token() {
        let token = NearToken::from_near(5);
        assert_eq!(token.into_near_token().unwrap(), NearToken::from_near(5));
    }

    #[test]
    fn test_into_near_token_from_str() {
        assert_eq!("5 NEAR".into_near_token().unwrap(), NearToken::from_near(5));
    }

    #[test]
    fn test_into_near_token_from_string() {
        let s = String::from("5 NEAR");
        assert_eq!(s.into_near_token().unwrap(), NearToken::from_near(5));
    }

    #[test]
    fn test_into_near_token_from_string_ref() {
        let s = String::from("5 NEAR");
        assert_eq!((&s).into_near_token().unwrap(), NearToken::from_near(5));
    }

    // ========================================================================
    // IntoGas tests
    // ========================================================================

    #[test]
    fn test_into_gas_from_gas() {
        let gas = Gas::from_tgas(30);
        assert_eq!(gas.into_gas().unwrap(), Gas::from_tgas(30));
    }

    #[test]
    fn test_into_gas_from_str() {
        assert_eq!("30 Tgas".into_gas().unwrap(), Gas::from_tgas(30));
    }

    #[test]
    fn test_into_gas_from_string() {
        let s = String::from("30 Tgas");
        assert_eq!(s.into_gas().unwrap(), Gas::from_tgas(30));
    }

    #[test]
    fn test_into_gas_from_string_ref() {
        let s = String::from("30 Tgas");
        assert_eq!((&s).into_gas().unwrap(), Gas::from_tgas(30));
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    #[test]
    fn test_near_token_default() {
        let default = NearToken::default();
        assert_eq!(default, NearToken::from_yoctonear(0));
    }

    #[test]
    fn test_gas_default_trait() {
        let default = Gas::default();
        assert_eq!(default, Gas::from_gas(0));
    }

    #[test]
    fn test_near_token_debug() {
        let token = NearToken::from_near(5);
        let debug = format!("{:?}", token);
        assert!(debug.contains("NearToken"));
    }

    #[test]
    fn test_gas_debug() {
        let gas = Gas::from_tgas(30);
        let debug = format!("{:?}", gas);
        assert!(debug.contains("NearGas"));
    }
}
