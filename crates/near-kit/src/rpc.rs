//! Low-level RPC access, transports, query builders, and response types.
//!
//! `RpcClient` includes the generic `RpcClient::call` escape hatch in
//! addition to typed NEAR RPC methods. The response and view types are
//! re-exported from `near-kit-primitives`.
//!
//! # Polling a transaction's progress
//!
//! Send at an early wait level, then poll `tx_status` for the partial outcome
//! carried by [`SendTxResponse`]:
//!
#![cfg_attr(feature = "rpc", doc = "```rust,no_run")]
#![cfg_attr(not(feature = "rpc"), doc = "```rust,ignore")]
//! # use near_kit::*;
//! # use near_kit::protocol::Action;
//! # use near_kit::transaction::Included;
//! # async fn example(alice: &SigningNear) -> Result<(), Error> {
//! let response = alice
//!     .wait_until::<Included>()
//!     .transaction(account!("bob.testnet"))
//!     .add_action(Action::transfer(NearToken::from_near(1)))
//!     .await?;
//!
//! // Later, poll for progress without waiting for finality. At early wait
//! // levels EXPERIMENTAL_tx_status still returns per-receipt data:
//! let status = alice
//!     .near()
//!     .tx_status(&response.transaction_hash, &response.sender_id)
//!     .wait_until::<Included>()
//!     .await?;
//! if let Some(outcome) = &status.outcome {
//!     for receipt_outcome in &outcome.receipts_outcome {
//!         // drive a progressive UI from each receipt's status
//!     }
//! }
//! # Ok(())
//! # }
//! ```

pub use crate::error::{RpcError, TimeoutCause};
#[doc(inline)]
pub use near_kit_primitives::rpc::OutcomeError;
#[doc(inline)]
pub use near_kit_primitives::rpc::{
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
    AccountExistsQuery, BalanceQuery, BoxFuture, RetryConfig, RpcClient, RpcTransport,
    TransactionStatusQuery, TransportResponse,
};
