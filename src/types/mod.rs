//! Core types for NEAR Protocol.
//!
//! This module provides hand-rolled types based on NEAR RPC responses,
//! designed for ergonomic use in client applications.

mod account;
mod action;
mod block_reference;
mod hash;
mod key;
mod rpc;
mod transaction;
mod units;

pub use account::AccountId;
pub use action::{
    AccessKey, AccessKeyPermission, Action, AddKeyAction, CreateAccountAction, DelegateAction,
    DeleteAccountAction, DeleteKeyAction, DeployContractAction, FunctionCallAction,
    FunctionCallPermission, NonDelegateAction, SignedDelegateAction, StakeAction, TransferAction,
};
pub use block_reference::{BlockReference, Finality, TxExecutionStatus};
pub use hash::CryptoHash;
pub use key::{KeyType, PublicKey, SecretKey, Signature};
pub use rpc::{
    AccessKeyDetails, AccessKeyInfoView, AccessKeyListView, AccessKeyPermissionView, AccessKeyView,
    AccountBalance, AccountView, ActionView, BlockHeaderView, BlockView, ChunkHeaderView,
    ExecutionOutcome, ExecutionOutcomeWithId, ExecutionStatus, FinalExecutionOutcome, GasPrice,
    NodeVersion, StatusResponse, SyncInfo, TransactionView, ViewFunctionResult,
};
pub use transaction::{SignedTransaction, Transaction};
pub use units::{Gas, IntoGas, IntoNearToken, NearToken};
