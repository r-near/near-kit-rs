//! Low-level RPC access, transports, query builders, and response types.
//!
//! `RpcClient` includes the generic `RpcClient::call` escape hatch in
//! addition to typed NEAR RPC methods.

pub use crate::error::RpcError;
pub use crate::types::{
    AccessKeyDetails, AccessKeyInfoView, AccessKeyListView, AccessKeyPermissionView, AccessKeyView,
    AccountBalance, AccountContractView, AccountView, ActionReceiptData, ActionView,
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
    VersionedDelegateActionPayloadView, ViewFunctionResult, ViewResult, ViewStateAllResult,
    ViewStateResult,
};

#[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
pub use crate::client::ReqwestTransport;
#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
pub use crate::client::WasiHttpTransport;
#[cfg(feature = "rpc")]
pub use crate::client::{
    AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, BoxFuture, ContractCodeQuery,
    GlobalContractQuery, RetryConfig, RpcClient, RpcTransport, SandboxNetwork,
    TransactionStatusQuery, TransportResponse, ViewCall, ViewCallBorsh,
};
