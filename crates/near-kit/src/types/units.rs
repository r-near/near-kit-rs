//! NEAR token amount and gas unit types — re-exported from upstream crates
//! with near-kit ergonomic extensions.

pub use near_gas::NearGas as Gas;
pub use near_token::NearToken;

use crate::error::{ParseAmountError, ParseGasError};

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

fn parse_upstream_near_token(
    value: &str,
    unit: &str,
    original: &str,
) -> Result<NearToken, ParseAmountError> {
    // Upstream requires a whole-number component before the decimal point.
    let numeric_value = value.trim();
    let normalized = if numeric_value.starts_with('.') {
        format!("0{numeric_value} {unit}")
    } else {
        format!("{numeric_value} {unit}")
    };

    normalized.parse().map_err(|error| match error {
        near_token::NearTokenError::InvalidTokensAmount(
            near_token::DecimalNumberParsingError::InvalidNumber(value),
        ) => ParseAmountError::InvalidNumber(value),
        near_token::NearTokenError::InvalidTokensAmount(
            near_token::DecimalNumberParsingError::LongWhole(_),
        ) => ParseAmountError::Overflow,
        near_token::NearTokenError::InvalidTokensAmount(
            near_token::DecimalNumberParsingError::LongFractional(_),
        ) => ParseAmountError::InvalidFormat(format!("Too many decimal places in: {original}")),
        // The unit is canonical here, so this only happens when malformed
        // numeric text makes upstream detect the unit boundary too early.
        near_token::NearTokenError::InvalidTokenUnit(_) => {
            ParseAmountError::InvalidNumber(numeric_value.to_string())
        }
    })
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
        return parse_upstream_near_token(value, "NEAR", s);
    }

    // "X milliNEAR" or "X mNEAR"
    if let Some(value) = s
        .strip_suffix(" milliNEAR")
        .or_else(|| s.strip_suffix(" mNEAR"))
    {
        return parse_upstream_near_token(value, "milliNEAR", s);
    }

    // "X yocto" or "X yoctoNEAR"
    if let Some(value) = s
        .strip_suffix(" yoctoNEAR")
        .or_else(|| s.strip_suffix(" yocto"))
    {
        return parse_upstream_near_token(value, "yoctoNEAR", s);
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
        return parse_upstream_gas(value, "Tgas", s);
    }

    // "X Ggas" or "X ggas" or "X GGas"
    if let Some(value) = s
        .strip_suffix(" Ggas")
        .or_else(|| s.strip_suffix(" ggas"))
        .or_else(|| s.strip_suffix(" GGas"))
    {
        return parse_upstream_gas(value, "Ggas", s);
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

fn parse_upstream_gas(value: &str, unit: &str, original: &str) -> Result<Gas, ParseGasError> {
    let numeric_value = value.trim();
    format!("{numeric_value} {unit}")
        .parse()
        .map_err(|error| match error {
            near_gas::NearGasError::IncorrectNumber(
                near_gas::DecimalNumberParsingError::InvalidNumber(value),
            ) => ParseGasError::InvalidNumber(value),
            near_gas::NearGasError::IncorrectNumber(
                near_gas::DecimalNumberParsingError::LongWhole(_),
            ) => ParseGasError::Overflow,
            near_gas::NearGasError::IncorrectNumber(
                near_gas::DecimalNumberParsingError::LongFractional(_),
            ) => ParseGasError::InvalidFormat(format!("Too many decimal places in: {original}")),
            // The unit is canonical here, so this only happens when malformed
            // numeric text makes upstream detect the unit boundary too early.
            near_gas::NearGasError::IncorrectUnit(_) => {
                ParseGasError::InvalidNumber(numeric_value.to_string())
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // NearToken parsing tests
    // ========================================================================

    #[test]
    fn test_near_token_parsing() {
        assert_eq!(parse_near_token("5 NEAR").unwrap(), NearToken::from_near(5));
        assert_eq!(
            parse_near_token("1.5 NEAR").unwrap(),
            NearToken::from_millinear(1500)
        );
        assert_eq!(
            parse_near_token("100 milliNEAR").unwrap(),
            NearToken::from_millinear(100)
        );
        assert_eq!(
            parse_near_token("1000 yocto").unwrap(),
            NearToken::from_yoctonear(1000)
        );
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
        assert_eq!(parse_gas("30 Tgas").unwrap(), Gas::from_tgas(30));
        assert_eq!(parse_gas("5 Ggas").unwrap(), Gas::from_ggas(5));
        assert_eq!(parse_gas("1000 gas").unwrap(), Gas::from_gas(1000));
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
            parse_near_token("100 mNEAR").unwrap(),
            NearToken::from_millinear(100)
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
            parse_near_token("0.5 NEAR").unwrap(),
            NearToken::from_millinear(500)
        );
        assert_eq!(
            parse_near_token(".25 NEAR").unwrap(),
            NearToken::from_millinear(250)
        );
    }

    #[test]
    fn test_near_token_precision_boundary() {
        let minimum = format!("0.{}1 NEAR", "0".repeat(23));
        assert_eq!(
            parse_near_token(&minimum).unwrap(),
            NearToken::from_yoctonear(1)
        );

        let excessive = format!("1.{}1 NEAR", "0".repeat(24));
        assert!(matches!(
            parse_near_token(&excessive),
            Err(ParseAmountError::InvalidFormat(_))
        ));
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
    fn test_near_token_parse_errors() {
        assert!(matches!(
            parse_near_token("abc NEAR"),
            Err(ParseAmountError::InvalidNumber(_))
        ));
        assert!(matches!(
            parse_near_token(&format!("{} NEAR", u128::MAX)),
            Err(ParseAmountError::Overflow)
        ));
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
    fn test_gas_parse_decimal() {
        assert_eq!(parse_gas("0.5 Tgas").unwrap(), Gas::from_ggas(500));
    }

    #[test]
    fn test_gas_precision_boundary() {
        let minimum = format!("0.{}1 Ggas", "0".repeat(8));
        assert_eq!(parse_gas(&minimum).unwrap(), Gas::from_gas(1));

        let excessive = format!("1.{}1 Ggas", "0".repeat(9));
        assert!(matches!(
            parse_gas(&excessive),
            Err(ParseGasError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_gas_parse_invalid_format() {
        assert!(matches!(
            parse_gas("30 teragas"),
            Err(ParseGasError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_gas_parse_errors() {
        assert!(matches!(
            parse_gas("abc Tgas"),
            Err(ParseGasError::InvalidNumber(_))
        ));
        assert!(matches!(
            parse_gas(&format!("{} Tgas", u64::MAX)),
            Err(ParseGasError::Overflow)
        ));
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
}
