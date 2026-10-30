//! NEAR protocol data types.
//!
//! This namespace contains actions, transactions, execution errors, and other
//! wire-level primitives. The small everyday primitives remain at the crate
//! root: [`AccountId`](crate::AccountId), [`NearToken`](crate::NearToken),
//! [`Gas`](crate::Gas), and [`CryptoHash`](crate::CryptoHash).

#[doc(inline)]
pub use near_kit_primitives::protocol::{
    AccessKey, AccessKeyPermission, Action, ActionError, ActionErrorKind, ActionsValidationError,
    AddKeyAction, CompilationError, CreateAccountAction, DecodeDelegateError, DelegateAction,
    DelegateActionV2, DeleteAccountAction, DeleteKeyAction, DeployContractAction,
    DeployGlobalContractAction, DepositCostFailureReason, DeterministicStateInitAction,
    FunctionCallAction, FunctionCallError, FunctionCallPermission, GasKeyInfo, GlobalContractId,
    HostError, InvalidAccessKeyError, InvalidTxError, MethodResolveError, NonDelegateAction, Nonce,
    NonceIndex, NonceMode, PrepareError, PublishMode, ReceiptValidationError, SignedDelegateAction,
    SignedTransaction, SignedTransactionV1, StakeAction, StateInit, StateInitV1, StorageError,
    Transaction, TransactionNonce, TransactionV1, TransferAction, TransferToGasKeyAction,
    TxExecutionError, UniversalStateInit, UniversalStateInitAction, UniversalStateInitV1,
    UnknownError, UseGlobalContractAction, VersionedDelegateActionPayload,
    VersionedSignedDelegateAction, VersionedTransaction, WasmTrap, WithdrawFromGasKeyAction,
    derive_deterministic_account_id,
};
#[doc(inline)]
pub use near_kit_primitives::protocol::{
    ActionViewConversionError, DecodeTransactionError, ParseAccountIdError, ParseHashError,
    StateInitError,
};

#[doc(inline)]
pub use near_kit_primitives::protocol::ChainId;

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts):
/// it is the type near-sdk uses too, so pass state inits between crates as
/// `RawStateInit`. Build one from near-kit's typed [`UniversalStateInit`] with
/// `From` or [`UniversalStateInit::to_raw`], and decode one with
/// [`UniversalStateInit::from_raw`]. The `UniversalStateInit` in the upstream
/// text below is near-global-contracts' own typed value, a different type,
/// whose `TryFrom` is lenient: never derive an account id from a value decoded
/// that way. For the unvalidated id of the bytes an action carries, as the
/// chain computes it, use [`UniversalStateInitAction::derive_account_id`].
///
/// ---
///
#[doc(inline)]
pub use near_kit_primitives::protocol::RawStateInit;
