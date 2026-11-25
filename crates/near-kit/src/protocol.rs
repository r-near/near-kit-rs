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
    EXTERNAL_CALL_METHOD, ExternalCall, ExternalCallChunkLimit, FunctionCallAction,
    FunctionCallError, FunctionCallPermission, GasKeyInfo, GlobalContractId, HostError,
    InvalidAccessKeyError, InvalidTxError, MethodResolveError, MissingTrieValue,
    MissingTrieValueContext, NonDelegateAction, Nonce, NonceIndex, NonceMode, PrepareError,
    PublishMode, ReceiptValidationError, SignedDelegateAction, SignedTransaction,
    SignedTransactionV1, StakeAction, StateInit, StateInitV1, StorageError, Transaction,
    TransactionNonce, TransactionV1, TransferAction, TransferToGasKeyAction, TxExecutionError,
    UniversalStateInitAction, UnknownError, UseGlobalContractAction,
    VersionedDelegateActionPayload, VersionedSignedDelegateAction, VersionedTransaction, WasmTrap,
    WithdrawFromGasKeyAction, derive_deterministic_account_id,
};
#[doc(inline)]
pub use near_kit_primitives::protocol::{
    ActionViewConversionError, DecodeTransactionError, ParseAccountIdError, ParseHashError,
    StateInitError,
};

#[doc(inline)]
pub use near_kit_primitives::protocol::ChainId;

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts),
/// as near-sdk does, so a state init passes between near-kit and contract code
/// as one type. Check bytes you were handed with
/// [`try_derive_account_id`](RawStateInit::try_derive_account_id) or
/// [`validate_canonical`](RawStateInit::validate_canonical), and forward them
/// unchanged. For the unvalidated id of the bytes an action carries, as the
/// chain computes it, use [`UniversalStateInitAction::derive_account_id`].
///
/// ---
///
#[doc(inline)]
pub use near_kit_primitives::protocol::RawStateInit;

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts),
/// as near-sdk does. Create the account with
/// [`Action::universal_state_init`], sent to
/// [`derive_account_id`](UniversalStateInit::derive_account_id).
///
/// Its `TryFrom<&RawStateInit>` is lenient: it accepts non-canonical bytes,
/// and re-encoding the result can change the account id. Never derive an id
/// from a value decoded that way; derive it from the bytes with
/// [`RawStateInit::try_derive_account_id`].
///
/// ---
///
#[doc(inline)]
pub use near_kit_primitives::protocol::UniversalStateInit;

/// near-kit re-exports this from
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts),
/// as near-sdk does. Start from `UniversalStateInitV1::default()`.
/// [`with_access_key`](UniversalStateInitV1::with_access_key) takes a
/// near-kit [`PublicKey`](crate::signer::PublicKey) or
/// [`PublicKeyHandle`](crate::signer::PublicKeyHandle) and stores an
/// ML-DSA-65 key as the hash the chain keys it by. `access_keys` holds
/// near-sdk's handle type; convert one to near-kit's with
/// `PublicKeyHandle::from`.
///
/// ---
///
#[doc(inline)]
pub use near_kit_primitives::protocol::UniversalStateInitV1;
