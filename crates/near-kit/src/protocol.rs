//! NEAR protocol data types.
//!
//! This namespace contains actions, transactions, execution errors, and other
//! wire-level primitives. The small everyday primitives remain at the crate
//! root: [`AccountId`](crate::AccountId), [`NearToken`](crate::NearToken),
//! [`Gas`](crate::Gas), and [`CryptoHash`](crate::CryptoHash).

#[doc(inline)]
pub use near_kit_primitives::protocol::{
    AccessKey, AccessKeyPermission, AccountIdRef, Action, ActionError, ActionErrorKind,
    ActionsValidationError, AddKeyAction, CompilationError, CreateAccountAction,
    DELEGATE_ACTION_PREFIX, DELEGATE_V2_ACTION_PREFIX, DelegateAction, DelegateActionV2,
    DelegateDecodeError, DeleteAccountAction, DeleteKeyAction, DeployContractAction,
    DeployGlobalContractAction, DepositCostFailureReason, DeterministicStateInitAction,
    FunctionCallAction, FunctionCallError, FunctionCallPermission, GasKeyInfo, GlobalContractId,
    HostError, IntoGas, IntoNearToken, InvalidAccessKeyError, InvalidTxError,
    MAX_NONCES_FOR_GAS_KEY, MethodResolveError, NonDelegateAction, Nonce, NonceIndex, NonceMode,
    PrepareError, PublishMode, RawStateInit, ReceiptValidationError, SignedDelegateAction,
    SignedTransaction, SignedTransactionV1, StakeAction, StateInit, StateInitExt, StateInitV1,
    StorageError, Transaction, TransactionNonce, TransactionV1, TransferAction,
    TransferToGasKeyAction, TryIntoAccountId, TryIntoGlobalContractId, TxExecutionError,
    UniversalStateInit, UniversalStateInitAction, UniversalStateInitV1, UnknownError,
    UseGlobalContractAction, VersionedDelegateActionPayload, VersionedSignedDelegateAction,
    VersionedTransaction, WasmTrap, WithdrawFromGasKeyAction,
};
#[doc(inline)]
pub use near_kit_primitives::protocol::{
    ActionViewConversionError, DecodeTransactionError, ParseAccountIdError, ParseAmountError,
    ParseGasError, ParseHashError, UniversalStateInitError,
};

#[doc(inline)]
pub use near_kit_primitives::protocol::ChainId;
