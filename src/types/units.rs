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
/// # Parsing
///
/// Supports parsing from strings with explicit units:
/// - `"5 NEAR"` or `"5 near"` - whole NEAR
/// - `"1.5 NEAR"` - decimal NEAR
/// - `"500 milliNEAR"` or `"500 mNEAR"` - milliNEAR
/// - `"1000 yocto"` or `"1000 yoctoNEAR"` - yoctoNEAR
///
/// Raw numbers are NOT accepted to prevent unit confusion.
///
/// # Examples
///
/// ```
/// use near_kit::NearToken;
///
/// let amount: NearToken = "5 NEAR".parse().unwrap();
/// assert_eq!(amount.as_yoctonear(), 5_000_000_000_000_000_000_000_000);
///
/// let decimal: NearToken = "1.5 NEAR".parse().unwrap();
/// assert_eq!(decimal.as_yoctonear(), 1_500_000_000_000_000_000_000_000);
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

/// Gas per teragas.
const GAS_PER_TGAS: u64 = 1_000_000_000_000;
/// Gas per gigagas.
const GAS_PER_GGAS: u64 = 1_000_000_000;

/// Gas units for NEAR transactions.
///
/// # Parsing
///
/// Supports parsing from strings:
/// - `"30 Tgas"` or `"30 tgas"` - teragas (10^12)
/// - `"5 Ggas"` or `"5 ggas"` - gigagas (10^9)
/// - `"1000000 gas"` - raw gas units
///
/// # Examples
///
/// ```
/// use near_kit::Gas;
///
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

    /// Default gas for function calls (30 Tgas).
    pub const DEFAULT: Self = Self::from_tgas(30);

    /// Maximum gas per transaction (300 Tgas).
    pub const MAX: Self = Self::from_tgas(300);

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
/// This allows methods to accept both string representations ("5 NEAR")
/// and direct NearToken values.
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
/// // Both work:
/// example("5 NEAR");
/// example(NearToken::from_near(5));
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
/// This allows methods to accept both string representations ("30 Tgas")
/// and direct Gas values.
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
/// // Both work:
/// example("30 Tgas");
/// example(Gas::from_tgas(30));
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
}
