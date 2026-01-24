//! Types for token operations.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::ParseAmountError;
use crate::types::NearToken;

// =============================================================================
// Fungible Token Types (NEP-141)
// =============================================================================

/// NEP-141 Fungible Token metadata.
///
/// This is returned by the `ft_metadata` view function on NEP-141 contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtMetadata {
    /// Standard specification version (e.g., "ft-1.0.0")
    pub spec: String,

    /// Human-readable token name (e.g., "USD Coin")
    pub name: String,

    /// Token symbol (e.g., "USDC")
    pub symbol: String,

    /// Number of decimal places for display (e.g., 6 for USDC, 24 for wNEAR)
    pub decimals: u8,

    /// Optional icon as a data URI (base64 SVG or image)
    pub icon: Option<String>,

    /// Optional URL to off-chain JSON metadata
    pub reference: Option<String>,

    /// Optional base64-encoded SHA-256 hash of the reference content
    pub reference_hash: Option<String>,
}

/// A fungible token amount with baked-in decimals and symbol for display.
///
/// `FtAmount` wraps a raw `u128` token amount along with the token's decimals
/// and symbol, allowing for human-readable formatting.
///
/// # Display
///
/// The `Display` implementation formats the amount with the correct number
/// of decimal places and includes the symbol:
///
/// ```
/// use near_kit::FtAmount;
///
/// let amount = FtAmount::new(1_500_000, 6, "USDC");
/// assert_eq!(format!("{}", amount), "1.5 USDC");
///
/// let amount = FtAmount::new(1_000_000_000_000_000_000_000_000, 24, "wNEAR");
/// assert_eq!(format!("{}", amount), "1 wNEAR");
/// ```
///
/// # Arithmetic
///
/// Arithmetic operations are supported but only between amounts of the same
/// token (same decimals AND symbol). Operations return `Option` to indicate
/// success or failure.
///
/// ```
/// use near_kit::FtAmount;
///
/// let a = FtAmount::new(1_000_000, 6, "USDC");
/// let b = FtAmount::new(500_000, 6, "USDC");
///
/// // Same token - works
/// let sum = a.checked_add(&b).unwrap();
/// assert_eq!(sum.raw(), 1_500_000);
///
/// // Different token - fails
/// let c = FtAmount::new(1_000_000, 6, "USDT");
/// assert!(a.checked_add(&c).is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtAmount {
    raw: u128,
    decimals: u8,
    symbol: String,
}

impl FtAmount {
    /// Create a new FtAmount from raw value with explicit decimals and symbol.
    pub fn new(raw: u128, decimals: u8, symbol: impl Into<String>) -> Self {
        Self {
            raw,
            decimals,
            symbol: symbol.into(),
        }
    }

    /// Create from raw value using token metadata.
    pub fn from_metadata(raw: u128, metadata: &FtMetadata) -> Self {
        Self::new(raw, metadata.decimals, &metadata.symbol)
    }

    /// Parse a human-readable decimal string like "1.5" into a raw amount.
    ///
    /// # Example
    ///
    /// ```
    /// use near_kit::FtAmount;
    ///
    /// let amount = FtAmount::parse("1.5", 6, "USDC").unwrap();
    /// assert_eq!(amount.raw(), 1_500_000);
    ///
    /// let amount = FtAmount::parse("100", 6, "USDC").unwrap();
    /// assert_eq!(amount.raw(), 100_000_000);
    /// ```
    pub fn parse(
        s: &str,
        decimals: u8,
        symbol: impl Into<String>,
    ) -> Result<Self, ParseAmountError> {
        let raw = parse_decimal_to_raw(s, decimals)?;
        Ok(Self::new(raw, decimals, symbol))
    }

    /// Get the raw token amount (in smallest units).
    pub fn raw(&self) -> u128 {
        self.raw
    }

    /// Get the number of decimal places.
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Get the token symbol.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Check if this amount is zero.
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    /// Format as a string without the symbol.
    pub fn format_amount(&self) -> String {
        format_raw_with_decimals(self.raw, self.decimals)
    }

    /// Checked addition - returns None if tokens don't match or overflow.
    pub fn checked_add(&self, other: &FtAmount) -> Option<FtAmount> {
        if self.decimals != other.decimals || self.symbol != other.symbol {
            return None;
        }
        self.raw.checked_add(other.raw).map(|raw| FtAmount {
            raw,
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Checked subtraction - returns None if tokens don't match or underflow.
    pub fn checked_sub(&self, other: &FtAmount) -> Option<FtAmount> {
        if self.decimals != other.decimals || self.symbol != other.symbol {
            return None;
        }
        self.raw.checked_sub(other.raw).map(|raw| FtAmount {
            raw,
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Checked multiplication by a scalar.
    pub fn checked_mul(&self, multiplier: u128) -> Option<FtAmount> {
        self.raw.checked_mul(multiplier).map(|raw| FtAmount {
            raw,
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Checked division by a scalar.
    pub fn checked_div(&self, divisor: u128) -> Option<FtAmount> {
        if divisor == 0 {
            return None;
        }
        Some(FtAmount {
            raw: self.raw / divisor,
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Saturating addition - clamps at max on overflow, returns None on token mismatch.
    pub fn saturating_add(&self, other: &FtAmount) -> Option<FtAmount> {
        if self.decimals != other.decimals || self.symbol != other.symbol {
            return None;
        }
        Some(FtAmount {
            raw: self.raw.saturating_add(other.raw),
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Saturating subtraction - clamps at 0 on underflow, returns None on token mismatch.
    pub fn saturating_sub(&self, other: &FtAmount) -> Option<FtAmount> {
        if self.decimals != other.decimals || self.symbol != other.symbol {
            return None;
        }
        Some(FtAmount {
            raw: self.raw.saturating_sub(other.raw),
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }
}

impl fmt::Display for FtAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.format_amount(), self.symbol)
    }
}

impl From<FtAmount> for u128 {
    fn from(amount: FtAmount) -> u128 {
        amount.raw
    }
}

impl From<&FtAmount> for u128 {
    fn from(amount: &FtAmount) -> u128 {
        amount.raw
    }
}

// =============================================================================
// Storage Types (NEP-145)
// =============================================================================

/// Storage balance bounds for a contract.
///
/// Returned by `storage_balance_bounds` view function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBalanceBounds {
    /// Minimum required storage deposit.
    pub min: NearToken,

    /// Maximum storage deposit (if limited).
    pub max: Option<NearToken>,
}

/// Storage balance for an account on a contract.
///
/// Returned by `storage_balance_of` view function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageBalance {
    /// Total storage deposit.
    pub total: NearToken,

    /// Available for withdrawal.
    pub available: NearToken,
}

// =============================================================================
// Non-Fungible Token Types (NEP-171/177)
// =============================================================================

/// NEP-177 NFT Contract metadata.
///
/// This is returned by the `nft_metadata` view function on NEP-171 contracts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftContractMetadata {
    /// Standard specification version (e.g., "nft-1.0.0")
    pub spec: String,

    /// Contract name (e.g., "Example NFT Collection")
    pub name: String,

    /// Contract symbol (e.g., "EXAMPLE")
    pub symbol: String,

    /// Optional icon as a data URI
    pub icon: Option<String>,

    /// Optional base URI for token metadata references
    pub base_uri: Option<String>,

    /// Optional URL to off-chain JSON metadata
    pub reference: Option<String>,

    /// Optional base64-encoded SHA-256 hash of the reference content
    pub reference_hash: Option<String>,
}

/// NEP-177 Token metadata.
///
/// Metadata for an individual NFT token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftTokenMetadata {
    /// Token title
    pub title: Option<String>,

    /// Token description
    pub description: Option<String>,

    /// URL to media file
    pub media: Option<String>,

    /// Base64-encoded SHA-256 hash of the media content
    pub media_hash: Option<String>,

    /// Number of copies (for limited editions)
    pub copies: Option<u64>,

    /// ISO 8601 datetime when issued
    pub issued_at: Option<String>,

    /// ISO 8601 datetime when expires
    pub expires_at: Option<String>,

    /// ISO 8601 datetime when starts being valid
    pub starts_at: Option<String>,

    /// ISO 8601 datetime when last updated
    pub updated_at: Option<String>,

    /// Extra arbitrary data (JSON string)
    pub extra: Option<String>,

    /// URL to off-chain JSON metadata
    pub reference: Option<String>,

    /// Base64-encoded SHA-256 hash of the reference content
    pub reference_hash: Option<String>,
}

/// NEP-171 Token.
///
/// A single non-fungible token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftToken {
    /// Unique token identifier within this contract
    pub token_id: String,

    /// Current owner of the token
    pub owner_id: String,

    /// Optional token metadata
    pub metadata: Option<NftTokenMetadata>,

    /// Optional approved accounts with their approval IDs (NEP-178)
    pub approved_account_ids: Option<HashMap<String, u64>>,
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Format a raw amount with the given number of decimals.
fn format_raw_with_decimals(raw: u128, decimals: u8) -> String {
    if decimals == 0 {
        return raw.to_string();
    }

    let divisor = 10u128.pow(decimals as u32);
    let whole = raw / divisor;
    let frac = raw % divisor;

    if frac == 0 {
        whole.to_string()
    } else {
        // Format with leading zeros, then trim trailing zeros
        let frac_str = format!("{:0>width$}", frac, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{}.{}", whole, trimmed)
    }
}

/// Parse a decimal string to a raw amount.
fn parse_decimal_to_raw(s: &str, decimals: u8) -> Result<u128, ParseAmountError> {
    let s = s.trim();

    if s.is_empty() {
        return Err(ParseAmountError::InvalidFormat(s.to_string()));
    }

    let parts: Vec<&str> = s.split('.').collect();

    match parts.len() {
        1 => {
            // No decimal point - multiply by 10^decimals
            let whole: u128 = parts[0]
                .parse()
                .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?;
            whole
                .checked_mul(10u128.pow(decimals as u32))
                .ok_or(ParseAmountError::Overflow)
        }
        2 => {
            // Has decimal point
            let whole: u128 = if parts[0].is_empty() {
                0
            } else {
                parts[0]
                    .parse()
                    .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?
            };

            let frac_str = parts[1];
            if frac_str.len() > decimals as usize {
                return Err(ParseAmountError::InvalidFormat(format!(
                    "Too many decimal places: {} has {} but max is {}",
                    s,
                    frac_str.len(),
                    decimals
                )));
            }

            // Pad fractional part with zeros
            let padded = format!("{:0<width$}", frac_str, width = decimals as usize);
            let frac: u128 = padded
                .parse()
                .map_err(|_| ParseAmountError::InvalidNumber(s.to_string()))?;

            let whole_shifted = whole
                .checked_mul(10u128.pow(decimals as u32))
                .ok_or(ParseAmountError::Overflow)?;

            whole_shifted
                .checked_add(frac)
                .ok_or(ParseAmountError::Overflow)
        }
        _ => Err(ParseAmountError::InvalidFormat(format!(
            "Multiple decimal points in: {}",
            s
        ))),
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ─── FtAmount Display Tests ───

    #[test]
    fn test_ft_amount_display_whole_number() {
        let amount = FtAmount::new(1_000_000, 6, "USDC");
        assert_eq!(format!("{}", amount), "1 USDC");
    }

    #[test]
    fn test_ft_amount_display_with_decimals() {
        let amount = FtAmount::new(1_500_000, 6, "USDC");
        assert_eq!(format!("{}", amount), "1.5 USDC");
    }

    #[test]
    fn test_ft_amount_display_small_decimals() {
        let amount = FtAmount::new(1_000_001, 6, "USDC");
        assert_eq!(format!("{}", amount), "1.000001 USDC");
    }

    #[test]
    fn test_ft_amount_display_trailing_zeros_trimmed() {
        let amount = FtAmount::new(1_100_000, 6, "USDC");
        assert_eq!(format!("{}", amount), "1.1 USDC");
    }

    #[test]
    fn test_ft_amount_display_zero() {
        let amount = FtAmount::new(0, 6, "USDC");
        assert_eq!(format!("{}", amount), "0 USDC");
    }

    #[test]
    fn test_ft_amount_display_24_decimals() {
        let amount = FtAmount::new(1_000_000_000_000_000_000_000_000, 24, "wNEAR");
        assert_eq!(format!("{}", amount), "1 wNEAR");
    }

    #[test]
    fn test_ft_amount_display_24_decimals_fractional() {
        let amount = FtAmount::new(1_500_000_000_000_000_000_000_000, 24, "wNEAR");
        assert_eq!(format!("{}", amount), "1.5 wNEAR");
    }

    #[test]
    fn test_ft_amount_display_zero_decimals() {
        let amount = FtAmount::new(42, 0, "TOKEN");
        assert_eq!(format!("{}", amount), "42 TOKEN");
    }

    #[test]
    fn test_ft_amount_display_fractional_only() {
        let amount = FtAmount::new(500_000, 6, "USDC");
        assert_eq!(format!("{}", amount), "0.5 USDC");
    }

    // ─── FtAmount Parsing Tests ───

    #[test]
    fn test_ft_amount_parse_whole_number() {
        let amount = FtAmount::parse("100", 6, "USDC").unwrap();
        assert_eq!(amount.raw(), 100_000_000);
    }

    #[test]
    fn test_ft_amount_parse_with_decimals() {
        let amount = FtAmount::parse("1.5", 6, "USDC").unwrap();
        assert_eq!(amount.raw(), 1_500_000);
    }

    #[test]
    fn test_ft_amount_parse_max_decimals() {
        let amount = FtAmount::parse("1.123456", 6, "USDC").unwrap();
        assert_eq!(amount.raw(), 1_123_456);
    }

    #[test]
    fn test_ft_amount_parse_fewer_decimals() {
        let amount = FtAmount::parse("1.1", 6, "USDC").unwrap();
        assert_eq!(amount.raw(), 1_100_000);
    }

    #[test]
    fn test_ft_amount_parse_fractional_only() {
        let amount = FtAmount::parse("0.5", 6, "USDC").unwrap();
        assert_eq!(amount.raw(), 500_000);
    }

    #[test]
    fn test_ft_amount_parse_leading_decimal() {
        let amount = FtAmount::parse(".5", 6, "USDC").unwrap();
        assert_eq!(amount.raw(), 500_000);
    }

    #[test]
    fn test_ft_amount_parse_too_many_decimals() {
        let result = FtAmount::parse("1.1234567", 6, "USDC");
        assert!(result.is_err());
    }

    #[test]
    fn test_ft_amount_parse_invalid() {
        assert!(FtAmount::parse("abc", 6, "USDC").is_err());
        assert!(FtAmount::parse("1.2.3", 6, "USDC").is_err());
        assert!(FtAmount::parse("", 6, "USDC").is_err());
    }

    // ─── FtAmount Arithmetic Tests ───

    #[test]
    fn test_ft_amount_checked_add_same_token() {
        let a = FtAmount::new(1_000_000, 6, "USDC");
        let b = FtAmount::new(500_000, 6, "USDC");
        let sum = a.checked_add(&b).unwrap();
        assert_eq!(sum.raw(), 1_500_000);
        assert_eq!(sum.symbol(), "USDC");
    }

    #[test]
    fn test_ft_amount_checked_add_different_symbol() {
        let a = FtAmount::new(1_000_000, 6, "USDC");
        let b = FtAmount::new(500_000, 6, "USDT");
        assert!(a.checked_add(&b).is_none());
    }

    #[test]
    fn test_ft_amount_checked_add_different_decimals() {
        let a = FtAmount::new(1_000_000, 6, "TOKEN");
        let b = FtAmount::new(500_000, 8, "TOKEN");
        assert!(a.checked_add(&b).is_none());
    }

    #[test]
    fn test_ft_amount_checked_add_overflow() {
        let a = FtAmount::new(u128::MAX, 6, "USDC");
        let b = FtAmount::new(1, 6, "USDC");
        assert!(a.checked_add(&b).is_none());
    }

    #[test]
    fn test_ft_amount_checked_sub() {
        let a = FtAmount::new(1_000_000, 6, "USDC");
        let b = FtAmount::new(400_000, 6, "USDC");
        let diff = a.checked_sub(&b).unwrap();
        assert_eq!(diff.raw(), 600_000);
    }

    #[test]
    fn test_ft_amount_checked_sub_underflow() {
        let a = FtAmount::new(400_000, 6, "USDC");
        let b = FtAmount::new(1_000_000, 6, "USDC");
        assert!(a.checked_sub(&b).is_none());
    }

    #[test]
    fn test_ft_amount_checked_mul() {
        let a = FtAmount::new(1_000_000, 6, "USDC");
        let result = a.checked_mul(3).unwrap();
        assert_eq!(result.raw(), 3_000_000);
    }

    #[test]
    fn test_ft_amount_checked_div() {
        let a = FtAmount::new(3_000_000, 6, "USDC");
        let result = a.checked_div(3).unwrap();
        assert_eq!(result.raw(), 1_000_000);
    }

    #[test]
    fn test_ft_amount_checked_div_by_zero() {
        let a = FtAmount::new(1_000_000, 6, "USDC");
        assert!(a.checked_div(0).is_none());
    }

    #[test]
    fn test_ft_amount_saturating_add() {
        let a = FtAmount::new(u128::MAX - 1, 6, "USDC");
        let b = FtAmount::new(10, 6, "USDC");
        let sum = a.saturating_add(&b).unwrap();
        assert_eq!(sum.raw(), u128::MAX);
    }

    #[test]
    fn test_ft_amount_saturating_sub() {
        let a = FtAmount::new(100, 6, "USDC");
        let b = FtAmount::new(200, 6, "USDC");
        let diff = a.saturating_sub(&b).unwrap();
        assert_eq!(diff.raw(), 0);
    }

    // ─── FtAmount Accessors Tests ───

    #[test]
    fn test_ft_amount_accessors() {
        let amount = FtAmount::new(1_500_000, 6, "USDC");
        assert_eq!(amount.raw(), 1_500_000);
        assert_eq!(amount.decimals(), 6);
        assert_eq!(amount.symbol(), "USDC");
        assert!(!amount.is_zero());
    }

    #[test]
    fn test_ft_amount_is_zero() {
        let zero = FtAmount::new(0, 6, "USDC");
        assert!(zero.is_zero());
    }

    #[test]
    fn test_ft_amount_into_u128() {
        let amount = FtAmount::new(1_500_000, 6, "USDC");
        let raw: u128 = amount.into();
        assert_eq!(raw, 1_500_000);
    }

    #[test]
    fn test_ft_amount_from_metadata() {
        let metadata = FtMetadata {
            spec: "ft-1.0.0".to_string(),
            name: "USD Coin".to_string(),
            symbol: "USDC".to_string(),
            decimals: 6,
            icon: None,
            reference: None,
            reference_hash: None,
        };
        let amount = FtAmount::from_metadata(1_500_000, &metadata);
        assert_eq!(format!("{}", amount), "1.5 USDC");
    }

    // ─── Format Amount Tests ───

    #[test]
    fn test_format_amount_without_symbol() {
        let amount = FtAmount::new(1_500_000, 6, "USDC");
        assert_eq!(amount.format_amount(), "1.5");
    }
}
