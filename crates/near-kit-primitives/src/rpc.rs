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
    BlockHeaderInnerLiteView, BlockHeaderView, BlockReference, BlockView, ChunkHeaderView,
    CongestionInfoView, ContractCodeView, CurrentEpochValidatorInfo, DataReceiptData,
    DataReceiverView, DelegateActionV2View, DelegateActionView, EpochValidatorInfo,
    ExecutionMetadata, ExecutionOutcome, ExecutionOutcomeWithId, ExecutionStatus,
    FinalExecutionOutcome, FinalExecutionStatus, Finality, GasKeyNoncesView, GasPrice,
    GasProfileEntry, GlobalContractIdentifierView, LightClientBlockLiteView, LightClientBlockView,
    MaintenanceWindow, MerkleDirection, MerklePathItem, NextEpochValidatorInfo, NodeVersion,
    RawTransactionResponse, Receipt, ReceiptContent, ReceiptToTxResponse, STORAGE_AMOUNT_PER_BYTE,
    SendTxResponse, SlashedValidator, StateChangeCauseView, StateChangeKindView,
    StateChangeValueView, StateChangeWithCauseView, StateItem, StatusResponse, SyncCheckpoint,
    SyncInfo, TransactionNonceView, TransactionView, TrieSplit, TxExecutionStatus, ValidatorInfo,
    ValidatorKickoutReason, ValidatorKickoutView, ValidatorStakeView, ValidatorStakeViewV1,
    VersionedDelegateActionPayloadView, ViewFunctionResult, ViewStateAllResult, ViewStateResult,
};
