//! Core types for NEAR Protocol.
//!
//! This module provides hand-rolled types based on NEAR RPC responses,
//! designed for ergonomic use in client applications.

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
    DeployContractAction, FunctionCallAction, FunctionCallPermission, NonDelegateAction,
    SignedDelegateAction, StakeAction, TransferAction, DELEGATE_ACTION_PREFIX,
};
pub use block_reference::{BlockReference, Finality, TxExecutionStatus};
pub use hash::CryptoHash;
pub use key::{KeyType, PublicKey, SecretKey, Signature};
pub use nep413::{
    generate_nonce, serialize_message, sign_message, verify_signature, SignMessageParams,
    SignedMessage, VerifyError, VerifyOptions, DEFAULT_MAX_AGE, NEP413_TAG,
};
pub use rpc::{
    AccessKeyDetails, AccessKeyInfoView, AccessKeyListView, AccessKeyPermissionView, AccessKeyView,
    AccountBalance, AccountView, ActionReceiptData, ActionView, BlockHeaderView, BlockView,
    ChunkHeaderView, DataReceiptData, ExecutionMetadata, ExecutionOutcome, ExecutionOutcomeWithId,
    ExecutionStatus, FinalExecutionOutcome, FinalExecutionOutcomeWithReceipts,
    FinalExecutionStatus, GasPrice, GasProfileEntry, MerkleDirection, MerklePathItem, NodeVersion,
    Receipt, ReceiptContent, StatusResponse, SyncInfo, TransactionView, ValidatorInfo,
    ValidatorProposal, ViewFunctionResult,
};
pub use transaction::{SignedTransaction, Transaction};
pub use units::{Gas, IntoGas, IntoNearToken, NearToken};
