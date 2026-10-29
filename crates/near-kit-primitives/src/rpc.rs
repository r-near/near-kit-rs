//! JSON-RPC request and response types: block references, finality, and the
//! views a NEAR node returns.
//!
//! This crate has no RPC client. These are the types `near-kit`'s client
//! sends and receives (re-exported there as `near_kit::rpc`), and they
//! deserialize from any NEAR RPC response you fetch some other way.

pub use crate::error::OutcomeError;
pub use crate::types::{
    AccessKeyDetails, AccessKeyInfoView, AccessKeyListView, AccessKeyPermissionView, AccessKeyView,
    AccountBalance, AccountContractView, AccountState, AccountView, ActionReceiptData, ActionView,
    BandwidthRequest, BandwidthRequestBitmap, BandwidthRequests, BandwidthRequestsV1, BlockEffects,
    BlockHeaderView, BlockReference, BlockView, ChunkHeaderView, CongestionInfoView,
    ContractCodeView, CurrentEpochValidatorInfo, DataReceiptData, DataReceiverView,
    DelegateActionV2View, DelegateActionView, EpochValidatorInfo, ExecutionMetadata,
    ExecutionOutcome, ExecutionOutcomeWithId, ExecutionStatus, FinalExecutionOutcome,
    FinalExecutionStatus, Finality, GasKeyNoncesView, GasPrice, GasProfileEntry,
    GlobalContractIdentifierView, MaintenanceWindow, MerkleDirection, MerklePathItem,
    NextEpochValidatorInfo, NodeVersion, Receipt, ReceiptContent, ReceiptToTxResponse,
    RpcTransactionResponse, STORAGE_AMOUNT_PER_BYTE, SendTxResponse, SlashedValidator,
    StateChangeKindView, StateItem, StatusResponse, SyncCheckpoint, SyncInfo, TransactionNonceView,
    TransactionView, TrieSplit, TxExecutionStatus, ValidatorInfo, ValidatorKickoutReason,
    ValidatorKickoutView, ValidatorStakeView, VersionedDelegateActionPayloadView,
    ViewFunctionResult, ViewStateAllResult, ViewStateResult,
};
