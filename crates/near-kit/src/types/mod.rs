//! Core types for NEAR Protocol.
//!
//! This module provides hand-rolled types based on NEAR RPC responses,
//! designed for ergonomic use in client applications.
//!
//! # Primary Types
//!
//! | Type | Description |
//! |------|-------------|
//! | [`AccountId`] | Validated NEAR account identifier |
//! | [`NearToken`] | Token amount with yoctoNEAR (10⁻²⁴) precision |
//! | [`Gas`] | Gas units for transactions |
//! | [`PublicKey`] | Ed25519 or Secp256k1 public key |
//! | [`SecretKey`] | Ed25519 or Secp256k1 secret key |
//! | [`CryptoHash`] | 32-byte SHA-256 hash (blocks, transactions) |
//!
//! # Amount Types
//!
//! [`NearToken`] and [`Gas`] support both typed constructors and string parsing:
//!
//! ```
//! use near_kit::{NearToken, Gas};
//!
//! // Typed constructors (compile-time safe, zero-cost)
//! let amount = NearToken::near(5);
//! let gas = Gas::tgas(30);
//!
//! // String parsing (for runtime input)
//! let amount: NearToken = "5 NEAR".parse().unwrap();
//! let gas: Gas = "30 Tgas".parse().unwrap();
//! ```
//!
//! # Block References
//!
//! [`BlockReference`] specifies which block state to query:
//!
//! - `BlockReference::Finality(Finality::Final)` — Fully finalized (default)
//! - `BlockReference::Finality(Finality::Optimistic)` — Latest optimistic
//! - `BlockReference::Height(12345)` — Specific block height
//! - `BlockReference::Hash(hash)` — Specific block hash
//!
//! # RPC Response Types
//!
//! Types for RPC responses include [`AccountView`], [`BlockView`],
//! [`FinalExecutionOutcome`], and others.

mod account;
mod action;
mod block_reference;
mod hash;
mod key;
pub mod nep413;
mod rpc;
mod transaction;
mod units;

pub use account::AccountId;
pub use action::{
    AccessKey, AccessKeyPermission, Action, AddKeyAction, CreateAccountAction,
    DecodeError as DelegateDecodeError, DelegateAction, DeleteAccountAction, DeleteKeyAction,
    DeployContractAction, DeployGlobalContractAction, DeterministicAccountStateInit,
    DeterministicAccountStateInitV1, DeterministicStateInitAction, FunctionCallAction,
    FunctionCallPermission, GlobalContractDeployMode, GlobalContractIdentifier, NonDelegateAction,
    SignedDelegateAction, StakeAction, TransferAction, UseGlobalContractAction,
    DELEGATE_ACTION_PREFIX,
};
pub use block_reference::{BlockReference, Finality, TxExecutionStatus};
pub use hash::CryptoHash;
pub use key::{
    generate_seed_phrase, KeyPair, KeyType, PublicKey, SecretKey, Signature, DEFAULT_HD_PATH,
    DEFAULT_WORD_COUNT,
};
pub use rpc::{
    AccessKeyDetails, AccessKeyInfoView, AccessKeyListView, AccessKeyPermissionView, AccessKeyView,
    AccountBalance, AccountView, ActionReceiptData, ActionView, BlockHeaderView, BlockView,
    ChunkHeaderView, DataReceiptData, ExecutionMetadata, ExecutionOutcome, ExecutionOutcomeWithId,
    ExecutionStatus, FinalExecutionOutcome, FinalExecutionOutcomeWithReceipts,
    FinalExecutionStatus, GasPrice, GasProfileEntry, MerkleDirection, MerklePathItem, NodeVersion,
    Receipt, ReceiptContent, StatusResponse, SyncInfo, TransactionView, ValidatorInfo,
    ValidatorProposal, ViewFunctionResult, STORAGE_AMOUNT_PER_BYTE,
};
pub use transaction::{SignedTransaction, Transaction};
pub use units::{Gas, IntoGas, IntoNearToken, NearToken};
