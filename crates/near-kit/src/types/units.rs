//! NEAR token amount and gas unit types.

use std::fmt::{self, Display};
use std::ops::{Add, Sub};
use std::str::FromStr;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{ParseAmountError, ParseGasError};

/// One yoctoNEAR (10^-24 NEAR).
const YOCTO_PER_NEAR: u128 = 1_000_000_000_000_000_000_000_000;
/// One milliNEAR in yoctoNEAR (10^-3 NEAR = 10^21 yocto).
const YOCTO_PER_MILLINEAR: u128 = 1_000_000_000_000_000_000_000;

/// A NEAR token amount with yoctoNEAR precision (10^-24 NEAR).
///
/// # Creating Amounts
///
/// Use the typed constructors for compile-time safety:
///
/// ```
/// use near_kit::NearToken;
///
/// // Preferred: typed constructors (const, zero-cost)
/// let five_near = NearToken::near(5);
/// let half_near = NearToken::millinear(500);
/// let one_yocto = NearToken::yocto(1);
///
/// // Also available: longer form
/// let amount = NearToken::from_near(5);
/// ```
///
/// # Parsing from Strings
///
/// String parsing is available for runtime input (CLI, config files):
/// - `"5 NEAR"` or `"5 near"` - whole NEAR
/// - `"1.5 NEAR"` - decimal NEAR
/// - `"500 milliNEAR"` or `"500 mNEAR"` - milliNEAR
/// - `"1000 yocto"` or `"1000 yoctoNEAR"` - yoctoNEAR
///
/// Raw numbers are NOT accepted to prevent unit confusion.
///
/// ```
/// use near_kit::NearToken;
///
/// // For runtime/user input only
/// let amount: NearToken = "5 NEAR".parse().unwrap();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NearToken(u128);

impl NearToken {
    /// Zero NEAR.
    pub const ZERO: Self = Self(0);
    /// One yoctoNEAR.
    pub const ONE_YOCTO: Self = Self(1);
    /// One milliNEAR.
    pub const ONE_MILLINEAR: Self = Self(YOCTO_PER_MILLINEAR);
    /// One NEAR.
    pub const ONE_NEAR: Self = Self(YOCTO_PER_NEAR);

    // ========================================================================
    // Short alias constructors (preferred)
    // ========================================================================

    /// Create from whole NEAR (short alias for `from_near`).
    ///
    /// # Example
    ///
    /// ```
    /// use near_kit::NearToken;
    ///
    /// let amount = NearToken::near(5);
    /// assert_eq!(amount, NearToken::from_near(5));
    /// ```
    pub const fn near(near: u128) -> Self {
        Self(near * YOCTO_PER_NEAR)
    }

    /// Create from milliNEAR (short alias for `from_millinear`).
    ///
    /// # Example
    ///
    /// ```
    /// use near_kit::NearToken;
    ///
    /// let amount = NearToken::millinear(500); // 0.5 NEAR
    /// assert_eq!(amount, NearToken::from_millinear(500));
    /// ```
    pub const fn millinear(millinear: u128) -> Self {
        Self(millinear * YOCTO_PER_MILLINEAR)
    }

    /// Create from yoctoNEAR (short alias for `from_yoctonear`).
    ///
    /// # Example
    ///
    /// ```
    /// use near_kit::NearToken;
    ///
    /// let amount = NearToken::yocto(1);
    /// assert_eq!(amount, NearToken::ONE_YOCTO);
    /// ```
    pub const fn yocto(yocto: u128) -> Self {
        Self(yocto)
    }

    // ========================================================================
    // Full-name constructors
    // ========================================================================

    /// Create from yoctoNEAR (10^-24 NEAR).
    pub const fn from_yoctonear(yocto: u128) -> Self {
        Self(yocto)
    }

    /// Create from milliNEAR (10^-3 NEAR).
    pub const fn from_millinear(millinear: u128) -> Self {
        Self(millinear * YOCTO_PER_MILLINEAR)
    }

    /// Create from whole NEAR.
    pub const fn from_near(near: u128) -> Self {
        Self(near * YOCTO_PER_NEAR)
    }

    /// Parse from decimal NEAR (e.g., 1.5 NEAR).
    pub fn from_near_decimal(s: &str) -> Result<Self, ParseAmountError> {
        let s = s.trim();

        if let Some(dot_pos) = s.find('.') {
            // Decimal NEAR
            let integer_part = &s[..dot_pos];
            let decimal_part = &s[dot_pos + 1..];

            // Parse integer part
            let integer: u128 = if integer_part.is_empty() {
                0
            } else {
                integer_part
                    .parse()
                    .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?
            };

            // Parse decimal part (pad or truncate to 24 digits)
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

            // Scale the decimal part
            let decimal_scale = 24 - decimal_str.len();
            let decimal_yocto = decimal * 10u128.pow(decimal_scale as u32);

            let total = integer
                .checked_mul(YOCTO_PER_NEAR)
                .and_then(|v| v.checked_add(decimal_yocto))
                .ok_or(ParseAmountError::Overflow)?;

            Ok(Self(total))
        } else {
            // Whole NEAR
            let near: u128 = s
                .parse()
                .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?;
            near.checked_mul(YOCTO_PER_NEAR)
                .map(Self)
                .ok_or(ParseAmountError::Overflow)
        }
    }

    /// Get the raw yoctoNEAR value.
    pub const fn as_yoctonear(&self) -> u128 {
        self.0
    }

    /// Get the value as NEAR (may lose precision).
    pub fn as_near_f64(&self) -> f64 {
        self.0 as f64 / YOCTO_PER_NEAR as f64
    }

    /// Get whole NEAR (truncated).
    pub const fn as_near(&self) -> u128 {
        self.0 / YOCTO_PER_NEAR
    }

    /// Checked addition.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Saturating addition.
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction.
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Check if zero.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl FromStr for NearToken {
    type Err = ParseAmountError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();

        // "X NEAR" or "X near"
        if let Some(value) = s.strip_suffix(" NEAR").or_else(|| s.strip_suffix(" near")) {
            return Self::from_near_decimal(value.trim());
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
                .map(Self)
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
            return Ok(Self(v));
        }

        // Bare number = error (ambiguous)
        if s.chars().all(|c| c.is_ascii_digit() || c == '.') {
            return Err(ParseAmountError::AmbiguousAmount(s.to_string()));
        }

        Err(ParseAmountError::InvalidFormat(s.to_string()))
    }
}

impl TryFrom<&str> for NearToken {
    type Error = ParseAmountError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl Display for NearToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == 0 {
            return write!(f, "0 NEAR");
        }

        let near = self.0 / YOCTO_PER_NEAR;
        let remainder = self.0 % YOCTO_PER_NEAR;

        if remainder == 0 {
            write!(f, "{} NEAR", near)
        } else {
            // Show up to 5 decimal places, trim trailing zeros
            let decimal = format!("{:024}", remainder);
            let decimal = decimal.trim_end_matches('0');
            let decimal_len = decimal.len().min(5);
            write!(f, "{}.{} NEAR", near, &decimal[..decimal_len])
        }
    }
}

impl Add for NearToken {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for NearToken {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

// Serde: serialize as string (yoctoNEAR) for JSON compatibility with NEAR RPC
impl Serialize for NearToken {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for NearToken {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s: String = serde::Deserialize::deserialize(d)?;
        Ok(Self(s.parse().map_err(serde::de::Error::custom)?))
    }
}

impl BorshSerialize for NearToken {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(&self.0, writer)
    }
}

impl BorshDeserialize for NearToken {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self(u128::deserialize_reader(reader)?))
    }
}

// ============================================================================
// Gas
// ============================================================================

/// Gas per petagas.
const GAS_PER_PGAS: u64 = 1_000_000_000_000_000;
/// Gas per teragas.
const GAS_PER_TGAS: u64 = 1_000_000_000_000;
/// Gas per gigagas.
const GAS_PER_GGAS: u64 = 1_000_000_000;

/// Gas units for NEAR transactions.
///
/// # Creating Gas Amounts
///
/// Use the typed constructors for compile-time safety:
///
/// ```
/// use near_kit::Gas;
///
/// // Preferred: short aliases (const, zero-cost)
/// let default_gas = Gas::tgas(30);
/// let more_gas = Gas::tgas(100);
///
/// // Common constants
/// let default = Gas::DEFAULT;  // 30 Tgas
/// let max = Gas::MAX;          // 1 Pgas (1000 Tgas)
/// ```
///
/// # Parsing from Strings
///
/// String parsing is available for runtime input:
/// - `"30 Tgas"` or `"30 tgas"` - teragas (10^12)
/// - `"5 Ggas"` or `"5 ggas"` - gigagas (10^9)
/// - `"1000000 gas"` - raw gas units
///
/// ```
/// use near_kit::Gas;
///
/// // For runtime/user input only
/// let gas: Gas = "30 Tgas".parse().unwrap();
/// assert_eq!(gas.as_gas(), 30_000_000_000_000);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Gas(u64);

impl Gas {
    /// Zero gas.
    pub const ZERO: Self = Self(0);
    /// One gigagas (10^9).
    pub const ONE_GGAS: Self = Self(GAS_PER_GGAS);
    /// One teragas (10^12).
    pub const ONE_TGAS: Self = Self(GAS_PER_TGAS);
    /// One petagas (10^15).
    pub const ONE_PGAS: Self = Self(GAS_PER_PGAS);

    /// Default gas for function calls (30 Tgas).
    pub const DEFAULT: Self = Self::from_tgas(30);

    /// Maximum gas per transaction (1 Pgas / 1000 Tgas).
    pub const MAX: Self = Self::from_tgas(1_000);

    // ========================================================================
    // Short alias constructors (preferred)
    // ========================================================================

    /// Create from teragas (short alias for `from_tgas`).
    ///
    /// # Example
    ///
    /// ```
    /// use near_kit::Gas;
    ///
    /// let gas = Gas::tgas(30);
    /// assert_eq!(gas, Gas::DEFAULT);
    /// ```
    pub const fn tgas(tgas: u64) -> Self {
        Self(tgas * GAS_PER_TGAS)
    }

    /// Create from gigagas (short alias for `from_ggas`).
    ///
    /// # Example
    ///
    /// ```
    /// use near_kit::Gas;
    ///
    /// let gas = Gas::ggas(5);
    /// assert_eq!(gas.as_ggas(), 5);
    /// ```
    pub const fn ggas(ggas: u64) -> Self {
        Self(ggas * GAS_PER_GGAS)
    }

    // ========================================================================
    // Full-name constructors
    // ========================================================================

    /// Create from raw gas units.
    pub const fn from_gas(gas: u64) -> Self {
        Self(gas)
    }

    /// Create from gigagas (10^9).
    pub const fn from_ggas(ggas: u64) -> Self {
        Self(ggas * GAS_PER_GGAS)
    }

    /// Create from teragas (10^12).
    pub const fn from_tgas(tgas: u64) -> Self {
        Self(tgas * GAS_PER_TGAS)
    }

    /// Get raw gas value.
    pub const fn as_gas(&self) -> u64 {
        self.0
    }

    /// Get value in teragas (truncated).
    pub const fn as_tgas(&self) -> u64 {
        self.0 / GAS_PER_TGAS
    }

    /// Get value in gigagas (truncated).
    pub const fn as_ggas(&self) -> u64 {
        self.0 / GAS_PER_GGAS
    }

    /// Checked addition.
    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction.
    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Check if zero.
    pub const fn is_zero(&self) -> bool {
        self.0 == 0
    }
}

impl FromStr for Gas {
    type Err = ParseGasError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
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
                .map(Self)
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
                .map(Self)
                .ok_or(ParseGasError::Overflow);
        }

        // "X gas"
        if let Some(value) = s.strip_suffix(" gas") {
            let v: u64 = value
                .trim()
                .parse()
                .map_err(|_| ParseGasError::InvalidNumber(s.to_string()))?;
            return Ok(Self(v));
        }

        Err(ParseGasError::InvalidFormat(s.to_string()))
    }
}

impl TryFrom<&str> for Gas {
    type Error = ParseGasError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl Display for Gas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let tgas = self.0 / GAS_PER_TGAS;
        if tgas > 0 && self.0 % GAS_PER_TGAS == 0 {
            write!(f, "{} Tgas", tgas)
        } else {
            write!(f, "{} gas", self.0)
        }
    }
}

impl Add for Gas {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Gas {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl Serialize for Gas {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for Gas {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v: u64 = serde::Deserialize::deserialize(d)?;
        Ok(Self(v))
    }
}

impl BorshSerialize for Gas {
    fn serialize<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()> {
        borsh::BorshSerialize::serialize(&self.0, writer)
    }
}

impl BorshDeserialize for Gas {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        Ok(Self(u64::deserialize_reader(reader)?))
    }
}

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
/// example(NearToken::near(5));
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
        self.parse()
    }
}

impl IntoNearToken for String {
    fn into_near_token(self) -> Result<NearToken, ParseAmountError> {
        self.parse()
    }
}

impl IntoNearToken for &String {
    fn into_near_token(self) -> Result<NearToken, ParseAmountError> {
        self.parse()
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
/// example(Gas::tgas(30));
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
        self.parse()
    }
}

impl IntoGas for String {
    fn into_gas(self) -> Result<Gas, ParseGasError> {
        self.parse()
    }
}

impl IntoGas for &String {
    fn into_gas(self) -> Result<Gas, ParseGasError> {
        self.parse()
    }
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
            "5 NEAR".parse::<NearToken>().unwrap().as_yoctonear(),
            5 * YOCTO_PER_NEAR
        );
        assert_eq!(
            "1.5 NEAR".parse::<NearToken>().unwrap().as_yoctonear(),
            YOCTO_PER_NEAR + YOCTO_PER_NEAR / 2
        );
        assert_eq!(
            "100 milliNEAR".parse::<NearToken>().unwrap().as_yoctonear(),
            100 * YOCTO_PER_MILLINEAR
        );
        assert_eq!(
            "1000 yocto".parse::<NearToken>().unwrap().as_yoctonear(),
            1000
        );
    }

    #[test]
    fn test_near_token_display() {
        assert_eq!(NearToken::ZERO.to_string(), "0 NEAR");
        assert_eq!(NearToken::from_near(5).to_string(), "5 NEAR");
        assert_eq!(NearToken::from_near(100).to_string(), "100 NEAR");
    }

    #[test]
    fn test_near_token_ambiguous() {
        assert!(matches!(
            "123".parse::<NearToken>(),
            Err(ParseAmountError::AmbiguousAmount(_))
        ));
    }

    #[test]
    fn test_gas_parsing() {
        assert_eq!(
            "30 Tgas".parse::<Gas>().unwrap().as_gas(),
            30 * GAS_PER_TGAS
        );
        assert_eq!("5 Ggas".parse::<Gas>().unwrap().as_gas(), 5 * GAS_PER_GGAS);
        assert_eq!("1000 gas".parse::<Gas>().unwrap().as_gas(), 1000);
    }

    #[test]
    fn test_gas_display() {
        assert_eq!(Gas::from_tgas(30).to_string(), "30 Tgas");
        assert_eq!(Gas::from_gas(1000).to_string(), "1000 gas");
    }

    #[test]
    fn test_gas_default() {
        assert_eq!(Gas::DEFAULT.as_tgas(), 30);
    }

    // ========================================================================
    // NearToken constructor tests
    // ========================================================================

    #[test]
    fn test_near_token_constructors() {
        // Short aliases
        assert_eq!(NearToken::near(5).as_yoctonear(), 5 * YOCTO_PER_NEAR);
        assert_eq!(
            NearToken::millinear(500).as_yoctonear(),
            500 * YOCTO_PER_MILLINEAR
        );
        assert_eq!(NearToken::yocto(1000).as_yoctonear(), 1000);

        // Full names
        assert_eq!(NearToken::from_near(5).as_yoctonear(), 5 * YOCTO_PER_NEAR);
        assert_eq!(
            NearToken::from_millinear(500).as_yoctonear(),
            500 * YOCTO_PER_MILLINEAR
        );
        assert_eq!(NearToken::from_yoctonear(1000).as_yoctonear(), 1000);
    }

    #[test]
    fn test_near_token_constants() {
        assert_eq!(NearToken::ZERO.as_yoctonear(), 0);
        assert_eq!(NearToken::ONE_YOCTO.as_yoctonear(), 1);
        assert_eq!(NearToken::ONE_MILLINEAR.as_yoctonear(), YOCTO_PER_MILLINEAR);
        assert_eq!(NearToken::ONE_NEAR.as_yoctonear(), YOCTO_PER_NEAR);
    }

    #[test]
    fn test_near_token_as_near() {
        assert_eq!(NearToken::near(5).as_near(), 5);
        assert_eq!(NearToken::millinear(500).as_near(), 0); // Truncated
        assert_eq!(NearToken::millinear(1500).as_near(), 1); // Truncated
    }

    #[test]
    fn test_near_token_as_near_f64() {
        let amount = NearToken::millinear(500);
        let f64_val = amount.as_near_f64();
        assert!((f64_val - 0.5).abs() < 0.0001);
    }

    #[test]
    fn test_near_token_is_zero() {
        assert!(NearToken::ZERO.is_zero());
        assert!(!NearToken::ONE_YOCTO.is_zero());
    }

    // ========================================================================
    // NearToken arithmetic tests
    // ========================================================================

    #[test]
    fn test_near_token_add() {
        let a = NearToken::near(5);
        let b = NearToken::near(3);
        assert_eq!((a + b).as_near(), 8);
    }

    #[test]
    fn test_near_token_sub() {
        let a = NearToken::near(5);
        let b = NearToken::near(3);
        assert_eq!((a - b).as_near(), 2);
    }

    #[test]
    fn test_near_token_checked_add() {
        let a = NearToken::near(5);
        let b = NearToken::near(3);
        assert_eq!(a.checked_add(b).unwrap().as_near(), 8);

        // Overflow
        let max = NearToken::from_yoctonear(u128::MAX);
        assert!(max.checked_add(NearToken::ONE_YOCTO).is_none());
    }

    #[test]
    fn test_near_token_checked_sub() {
        let a = NearToken::near(5);
        let b = NearToken::near(3);
        assert_eq!(a.checked_sub(b).unwrap().as_near(), 2);

        // Underflow
        assert!(b.checked_sub(a).is_none());
    }

    #[test]
    fn test_near_token_saturating_add() {
        let a = NearToken::near(5);
        let b = NearToken::near(3);
        assert_eq!(a.saturating_add(b).as_near(), 8);

        // Saturates at max
        let max = NearToken::from_yoctonear(u128::MAX);
        assert_eq!(max.saturating_add(NearToken::ONE_YOCTO), max);
    }

    #[test]
    fn test_near_token_saturating_sub() {
        let a = NearToken::near(5);
        let b = NearToken::near(3);
        assert_eq!(a.saturating_sub(b).as_near(), 2);

        // Saturates at zero
        assert_eq!(b.saturating_sub(a), NearToken::ZERO);
    }

    // ========================================================================
    // NearToken parsing edge cases
    // ========================================================================

    #[test]
    fn test_near_token_parse_lowercase() {
        assert_eq!("5 near".parse::<NearToken>().unwrap().as_near(), 5);
    }

    #[test]
    fn test_near_token_parse_mnear() {
        assert_eq!(
            "100 mNEAR".parse::<NearToken>().unwrap().as_yoctonear(),
            100 * YOCTO_PER_MILLINEAR
        );
    }

    #[test]
    fn test_near_token_parse_yoctonear() {
        assert_eq!(
            "12345 yoctoNEAR"
                .parse::<NearToken>()
                .unwrap()
                .as_yoctonear(),
            12345
        );
    }

    #[test]
    fn test_near_token_parse_decimal_near() {
        assert_eq!(
            "0.5 NEAR".parse::<NearToken>().unwrap().as_yoctonear(),
            YOCTO_PER_NEAR / 2
        );
        assert_eq!(
            ".25 NEAR".parse::<NearToken>().unwrap().as_yoctonear(),
            YOCTO_PER_NEAR / 4
        );
    }

    #[test]
    fn test_near_token_parse_with_whitespace() {
        assert_eq!("  5 NEAR  ".parse::<NearToken>().unwrap().as_near(), 5);
    }

    #[test]
    fn test_near_token_parse_invalid_format() {
        assert!(matches!(
            "5 ETH".parse::<NearToken>(),
            Err(ParseAmountError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_near_token_parse_invalid_number() {
        assert!(matches!(
            "abc NEAR".parse::<NearToken>(),
            Err(ParseAmountError::InvalidNumber(_))
        ));
    }

    #[test]
    fn test_near_token_try_from_str() {
        let token = NearToken::try_from("5 NEAR").unwrap();
        assert_eq!(token.as_near(), 5);
    }

    // ========================================================================
    // NearToken serde tests
    // ========================================================================

    #[test]
    fn test_near_token_serde_roundtrip() {
        let amount = NearToken::near(5);
        let json = serde_json::to_string(&amount).unwrap();
        // Should serialize as string (yoctoNEAR)
        assert_eq!(json, format!("\"{}\"", amount.as_yoctonear()));

        let parsed: NearToken = serde_json::from_str(&json).unwrap();
        assert_eq!(amount, parsed);
    }

    #[test]
    fn test_near_token_borsh_roundtrip() {
        let amount = NearToken::near(10);
        let bytes = borsh::to_vec(&amount).unwrap();
        let parsed: NearToken = borsh::from_slice(&bytes).unwrap();
        assert_eq!(amount, parsed);
    }

    // ========================================================================
    // NearToken display tests (fractional)
    // ========================================================================

    #[test]
    fn test_near_token_display_fractional() {
        // 1.5 NEAR
        let amount = NearToken::from_yoctonear(YOCTO_PER_NEAR + YOCTO_PER_NEAR / 2);
        let display = amount.to_string();
        assert!(display.contains("1.5") || display.contains("1."));
        assert!(display.contains("NEAR"));
    }

    // ========================================================================
    // NearToken comparison tests
    // ========================================================================

    #[test]
    fn test_near_token_ord() {
        let small = NearToken::near(1);
        let large = NearToken::near(10);
        assert!(small < large);
        assert!(large > small);
        assert!(small <= small);
        assert!(small >= small);
    }

    #[test]
    fn test_near_token_eq() {
        let a = NearToken::near(5);
        let b = NearToken::millinear(5000);
        assert_eq!(a, b);
    }

    #[test]
    fn test_near_token_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(NearToken::near(1));
        set.insert(NearToken::near(2));
        assert!(set.contains(&NearToken::near(1)));
        assert!(!set.contains(&NearToken::near(3)));
    }

    // ========================================================================
    // Gas tests
    // ========================================================================

    #[test]
    fn test_gas_constructors() {
        assert_eq!(Gas::tgas(30).as_gas(), 30 * GAS_PER_TGAS);
        assert_eq!(Gas::ggas(5).as_gas(), 5 * GAS_PER_GGAS);
        assert_eq!(Gas::from_gas(1000).as_gas(), 1000);
        assert_eq!(Gas::from_tgas(30).as_gas(), 30 * GAS_PER_TGAS);
        assert_eq!(Gas::from_ggas(5).as_gas(), 5 * GAS_PER_GGAS);
    }

    #[test]
    fn test_gas_constants() {
        assert_eq!(Gas::ZERO.as_gas(), 0);
        assert_eq!(Gas::ONE_GGAS.as_gas(), GAS_PER_GGAS);
        assert_eq!(Gas::ONE_TGAS.as_gas(), GAS_PER_TGAS);
        assert_eq!(Gas::ONE_PGAS.as_gas(), GAS_PER_PGAS);
        assert_eq!(Gas::DEFAULT.as_tgas(), 30);
        assert_eq!(Gas::MAX.as_tgas(), 1_000);
    }

    #[test]
    fn test_gas_as_accessors() {
        let gas = Gas::tgas(30);
        assert_eq!(gas.as_tgas(), 30);
        assert_eq!(gas.as_ggas(), 30_000);
        assert_eq!(gas.as_gas(), 30 * GAS_PER_TGAS);
    }

    #[test]
    fn test_gas_is_zero() {
        assert!(Gas::ZERO.is_zero());
        assert!(!Gas::ONE_GGAS.is_zero());
    }

    #[test]
    fn test_gas_add() {
        let a = Gas::tgas(10);
        let b = Gas::tgas(20);
        assert_eq!((a + b).as_tgas(), 30);
    }

    #[test]
    fn test_gas_sub() {
        let a = Gas::tgas(30);
        let b = Gas::tgas(10);
        assert_eq!((a - b).as_tgas(), 20);
    }

    #[test]
    fn test_gas_checked_add() {
        let a = Gas::tgas(10);
        let b = Gas::tgas(20);
        assert_eq!(a.checked_add(b).unwrap().as_tgas(), 30);

        // Overflow
        let max = Gas::from_gas(u64::MAX);
        assert!(max.checked_add(Gas::from_gas(1)).is_none());
    }

    #[test]
    fn test_gas_checked_sub() {
        let a = Gas::tgas(30);
        let b = Gas::tgas(10);
        assert_eq!(a.checked_sub(b).unwrap().as_tgas(), 20);

        // Underflow
        assert!(b.checked_sub(a).is_none());
    }

    #[test]
    fn test_gas_parse_tgas_variants() {
        assert_eq!("30 Tgas".parse::<Gas>().unwrap().as_tgas(), 30);
        assert_eq!("30 tgas".parse::<Gas>().unwrap().as_tgas(), 30);
        assert_eq!("30 TGas".parse::<Gas>().unwrap().as_tgas(), 30);
    }

    #[test]
    fn test_gas_parse_ggas_variants() {
        assert_eq!("5 Ggas".parse::<Gas>().unwrap().as_ggas(), 5);
        assert_eq!("5 ggas".parse::<Gas>().unwrap().as_ggas(), 5);
        assert_eq!("5 GGas".parse::<Gas>().unwrap().as_ggas(), 5);
    }

    #[test]
    fn test_gas_parse_invalid_format() {
        assert!(matches!(
            "30 teragas".parse::<Gas>(),
            Err(ParseGasError::InvalidFormat(_))
        ));
    }

    #[test]
    fn test_gas_parse_invalid_number() {
        assert!(matches!(
            "abc Tgas".parse::<Gas>(),
            Err(ParseGasError::InvalidNumber(_))
        ));
    }

    #[test]
    fn test_gas_try_from_str() {
        let gas = Gas::try_from("30 Tgas").unwrap();
        assert_eq!(gas.as_tgas(), 30);
    }

    #[test]
    fn test_gas_serde_roundtrip() {
        let gas = Gas::tgas(30);
        let json = serde_json::to_string(&gas).unwrap();
        let parsed: Gas = serde_json::from_str(&json).unwrap();
        assert_eq!(gas, parsed);
    }

    #[test]
    fn test_gas_borsh_roundtrip() {
        let gas = Gas::tgas(30);
        let bytes = borsh::to_vec(&gas).unwrap();
        let parsed: Gas = borsh::from_slice(&bytes).unwrap();
        assert_eq!(gas, parsed);
    }

    #[test]
    fn test_gas_ord() {
        let small = Gas::tgas(10);
        let large = Gas::tgas(100);
        assert!(small < large);
    }

    // ========================================================================
    // IntoNearToken tests
    // ========================================================================

    #[test]
    fn test_into_near_token_from_near_token() {
        let token = NearToken::near(5);
        assert_eq!(token.into_near_token().unwrap(), NearToken::near(5));
    }

    #[test]
    fn test_into_near_token_from_str() {
        assert_eq!("5 NEAR".into_near_token().unwrap(), NearToken::near(5));
    }

    #[test]
    fn test_into_near_token_from_string() {
        let s = String::from("5 NEAR");
        assert_eq!(s.into_near_token().unwrap(), NearToken::near(5));
    }

    #[test]
    fn test_into_near_token_from_string_ref() {
        let s = String::from("5 NEAR");
        assert_eq!((&s).into_near_token().unwrap(), NearToken::near(5));
    }

    // ========================================================================
    // IntoGas tests
    // ========================================================================

    #[test]
    fn test_into_gas_from_gas() {
        let gas = Gas::tgas(30);
        assert_eq!(gas.into_gas().unwrap(), Gas::tgas(30));
    }

    #[test]
    fn test_into_gas_from_str() {
        assert_eq!("30 Tgas".into_gas().unwrap(), Gas::tgas(30));
    }

    #[test]
    fn test_into_gas_from_string() {
        let s = String::from("30 Tgas");
        assert_eq!(s.into_gas().unwrap(), Gas::tgas(30));
    }

    #[test]
    fn test_into_gas_from_string_ref() {
        let s = String::from("30 Tgas");
        assert_eq!((&s).into_gas().unwrap(), Gas::tgas(30));
    }

    // ========================================================================
    // Edge case tests
    // ========================================================================

    #[test]
    fn test_near_token_default() {
        let default = NearToken::default();
        assert_eq!(default, NearToken::ZERO);
    }

    #[test]
    fn test_gas_default_trait() {
        let default = Gas::default();
        assert_eq!(default, Gas::ZERO);
    }

    #[test]
    fn test_near_token_debug() {
        let token = NearToken::near(5);
        let debug = format!("{:?}", token);
        assert!(debug.contains("NearToken"));
    }

    #[test]
    fn test_gas_debug() {
        let gas = Gas::tgas(30);
        let debug = format!("{:?}", gas);
        assert!(debug.contains("Gas"));
    }

    #[test]
    fn test_gas_display_non_tgas_multiple() {
        // When gas is not a clean Tgas multiple
        let gas = Gas::from_gas(1500);
        assert_eq!(gas.to_string(), "1500 gas");
    }
}
