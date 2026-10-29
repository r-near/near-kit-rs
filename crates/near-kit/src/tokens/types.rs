//! Types for token operations.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::NearToken;

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
