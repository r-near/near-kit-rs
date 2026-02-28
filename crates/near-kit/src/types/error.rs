//! Typed error types for NEAR transaction execution.
//!
//! These types mirror the error hierarchy returned by NEAR RPC in
//! `ExecutionStatus::Failure`, replacing opaque `serde_json::Value`.

use serde::Deserialize;

use super::{AccountId, CryptoHash, Gas, NearToken, PublicKey};

// ============================================================================
// Top-level error
// ============================================================================

/// Error returned by NEAR RPC when a transaction or receipt fails.
#[derive(Debug, Clone, Deserialize)]
pub enum TxExecutionError {
    /// An error happened during action execution.
    ActionError(ActionError),
    /// An error happened during transaction validation.
    InvalidTxError(InvalidTxError),
}

impl std::fmt::Display for TxExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ActionError(e) => write!(f, "ActionError: {e}"),
            Self::InvalidTxError(e) => write!(f, "InvalidTxError: {e}"),
        }
    }
}

impl std::error::Error for TxExecutionError {}

// ============================================================================
// Action errors
// ============================================================================

/// An error that occurred during action execution.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionError {
    /// Index of the failed action in the transaction.
    /// Not defined if kind is `ActionErrorKind::LackBalanceForState`.
    #[serde(default)]
    pub index: Option<u64>,
    /// The kind of action error.
    pub kind: ActionErrorKind,
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.index {
            Some(i) => write!(f, "action #{i}: {}", self.kind),
            None => write!(f, "{}", self.kind),
        }
    }
}

/// Specific kind of action error.
#[derive(Debug, Clone, Deserialize)]
pub enum ActionErrorKind {
    AccountAlreadyExists {
        account_id: AccountId,
    },
    AccountDoesNotExist {
        account_id: AccountId,
    },
    CreateAccountOnlyByRegistrar {
        account_id: AccountId,
        predecessor_id: AccountId,
        registrar_account_id: AccountId,
    },
    CreateAccountNotAllowed {
        account_id: AccountId,
        predecessor_id: AccountId,
    },
    ActorNoPermission {
        account_id: AccountId,
        actor_id: AccountId,
    },
    DeleteKeyDoesNotExist {
        account_id: AccountId,
        public_key: PublicKey,
    },
    AddKeyAlreadyExists {
        account_id: AccountId,
        public_key: PublicKey,
    },
    DeleteAccountStaking {
        account_id: AccountId,
    },
    LackBalanceForState {
        account_id: AccountId,
        amount: NearToken,
    },
    TriesToUnstake {
        account_id: AccountId,
    },
    TriesToStake {
        account_id: AccountId,
        balance: NearToken,
        locked: NearToken,
        stake: NearToken,
    },
    InsufficientStake {
        account_id: AccountId,
        minimum_stake: NearToken,
        stake: NearToken,
    },
    FunctionCallError(FunctionCallError),
    NewReceiptValidationError(ReceiptValidationError),
    OnlyImplicitAccountCreationAllowed {
        account_id: AccountId,
    },
    DeleteAccountWithLargeState {
        account_id: AccountId,
    },
    DelegateActionInvalidSignature,
    DelegateActionSenderDoesNotMatchTxReceiver {
        receiver_id: AccountId,
        sender_id: AccountId,
    },
    DelegateActionExpired,
    DelegateActionAccessKeyError(InvalidAccessKeyError),
    DelegateActionInvalidNonce {
        ak_nonce: u64,
        delegate_nonce: u64,
    },
    DelegateActionNonceTooLarge {
        delegate_nonce: u64,
        upper_bound: u64,
    },
    GlobalContractDoesNotExist {
        identifier: serde_json::Value,
    },
    GasKeyDoesNotExist {
        account_id: AccountId,
        public_key: PublicKey,
    },
    InsufficientGasKeyBalance {
        account_id: AccountId,
        balance: NearToken,
        public_key: PublicKey,
        required: NearToken,
    },
    GasKeyBalanceTooHigh {
        account_id: AccountId,
        balance: NearToken,
        #[serde(default)]
        public_key: Option<PublicKey>,
    },
}

impl std::fmt::Display for ActionErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccountAlreadyExists { account_id } => {
                write!(f, "account {account_id} already exists")
            }
            Self::AccountDoesNotExist { account_id } => {
                write!(f, "account {account_id} does not exist")
            }
            Self::LackBalanceForState { account_id, amount } => {
                write!(f, "account {account_id} lacks {amount} for state")
            }
            Self::FunctionCallError(e) => write!(f, "{e}"),
            _ => write!(f, "{self:?}"),
        }
    }
}

// ============================================================================
// Transaction validation errors
// ============================================================================

/// An error during transaction validation (before execution).
#[derive(Debug, Clone, Deserialize)]
pub enum InvalidTxError {
    InvalidAccessKeyError(InvalidAccessKeyError),
    InvalidSignerId {
        signer_id: String,
    },
    SignerDoesNotExist {
        signer_id: AccountId,
    },
    InvalidNonce {
        ak_nonce: u64,
        tx_nonce: u64,
    },
    NonceTooLarge {
        tx_nonce: u64,
        upper_bound: u64,
    },
    InvalidReceiverId {
        receiver_id: String,
    },
    InvalidSignature,
    NotEnoughBalance {
        balance: NearToken,
        cost: NearToken,
        signer_id: AccountId,
    },
    LackBalanceForState {
        amount: NearToken,
        signer_id: AccountId,
    },
    CostOverflow,
    InvalidChain,
    Expired,
    ActionsValidation(ActionsValidationError),
    TransactionSizeExceeded {
        limit: u64,
        size: u64,
    },
    InvalidTransactionVersion,
    StorageError(StorageError),
    ShardCongested {
        congestion_level: f64,
        shard_id: u32,
    },
    ShardStuck {
        missed_chunks: u64,
        shard_id: u32,
    },
    InvalidNonceIndex {
        num_nonces: u16,
        #[serde(default)]
        tx_nonce_index: Option<u16>,
    },
    NotEnoughGasKeyBalance {
        balance: NearToken,
        cost: NearToken,
        signer_id: AccountId,
    },
    NotEnoughBalanceForDeposit {
        balance: NearToken,
        cost: NearToken,
        reason: DepositCostFailureReason,
        signer_id: AccountId,
    },
}

impl std::fmt::Display for InvalidTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSignature => write!(f, "invalid signature"),
            Self::NotEnoughBalance {
                signer_id, cost, ..
            } => write!(
                f,
                "{signer_id} does not have enough balance to cover {cost}"
            ),
            Self::InvalidNonce {
                ak_nonce, tx_nonce, ..
            } => write!(
                f,
                "invalid nonce: tx nonce {tx_nonce}, access key nonce {ak_nonce}"
            ),
            Self::Expired => write!(f, "transaction has expired"),
            Self::ShardCongested { shard_id, .. } => write!(f, "shard {shard_id} is congested"),
            _ => write!(f, "{self:?}"),
        }
    }
}

// ============================================================================
// Access key errors
// ============================================================================

/// Error related to access key validation.
#[derive(Debug, Clone, Deserialize)]
pub enum InvalidAccessKeyError {
    AccessKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
    },
    ReceiverMismatch {
        ak_receiver: String,
        tx_receiver: AccountId,
    },
    MethodNameMismatch {
        method_name: String,
    },
    RequiresFullAccess,
    NotEnoughAllowance {
        account_id: AccountId,
        allowance: NearToken,
        cost: NearToken,
        public_key: PublicKey,
    },
    DepositWithFunctionCall,
}

// ============================================================================
// Function call errors (VM / contract)
// ============================================================================

/// An error during function call execution.
#[derive(Debug, Clone, Deserialize)]
pub enum FunctionCallError {
    WasmUnknownError,
    #[serde(rename = "_EVMError")]
    EvmError,
    CompilationError(CompilationError),
    LinkError {
        msg: String,
    },
    MethodResolveError(MethodResolveError),
    WasmTrap(WasmTrap),
    HostError(HostError),
    ExecutionError(String),
}

impl std::fmt::Display for FunctionCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExecutionError(msg) => write!(f, "execution error: {msg}"),
            Self::HostError(e) => write!(f, "host error: {e:?}"),
            Self::WasmTrap(e) => write!(f, "wasm trap: {e:?}"),
            Self::CompilationError(e) => write!(f, "compilation error: {e:?}"),
            Self::MethodResolveError(e) => write!(f, "method resolve error: {e:?}"),
            Self::LinkError { msg } => write!(f, "link error: {msg}"),
            _ => write!(f, "{self:?}"),
        }
    }
}

/// Wasm compilation error.
#[derive(Debug, Clone, Deserialize)]
pub enum CompilationError {
    CodeDoesNotExist { account_id: AccountId },
    PrepareError(PrepareError),
    WasmerCompileError { msg: String },
}

/// Error preparing a Wasm module.
#[derive(Debug, Clone, Deserialize)]
pub enum PrepareError {
    Serialization,
    Deserialization,
    InternalMemoryDeclared,
    GasInstrumentation,
    StackHeightInstrumentation,
    Instantiate,
    Memory,
    TooManyFunctions,
    TooManyLocals,
    TooManyTables,
    TooManyTableElements,
}

/// Error resolving a method in Wasm.
#[derive(Debug, Clone, Deserialize)]
pub enum MethodResolveError {
    MethodEmptyName,
    MethodNotFound,
    MethodInvalidSignature,
}

/// A trap during Wasm execution.
#[derive(Debug, Clone, Deserialize)]
pub enum WasmTrap {
    Unreachable,
    IncorrectCallIndirectSignature,
    MemoryOutOfBounds,
    #[serde(rename = "CallIndirectOOB")]
    CallIndirectOob,
    IllegalArithmetic,
    MisalignedAtomicAccess,
    IndirectCallToNull,
    StackOverflow,
    GenericTrap,
}

/// Error from a host function call.
#[derive(Debug, Clone, Deserialize)]
pub enum HostError {
    #[serde(rename = "BadUTF16")]
    BadUtf16,
    #[serde(rename = "BadUTF8")]
    BadUtf8,
    GasExceeded,
    GasLimitExceeded,
    BalanceExceeded,
    EmptyMethodName,
    GuestPanic {
        panic_msg: String,
    },
    IntegerOverflow,
    InvalidPromiseIndex {
        promise_idx: u64,
    },
    CannotAppendActionToJointPromise,
    CannotReturnJointPromise,
    InvalidPromiseResultIndex {
        result_idx: u64,
    },
    InvalidRegisterId {
        register_id: u64,
    },
    IteratorWasInvalidated {
        iterator_index: u64,
    },
    MemoryAccessViolation,
    InvalidReceiptIndex {
        receipt_index: u64,
    },
    InvalidIteratorIndex {
        iterator_index: u64,
    },
    InvalidAccountId,
    InvalidMethodName,
    InvalidPublicKey,
    ProhibitedInView {
        method_name: String,
    },
    NumberOfLogsExceeded {
        limit: u64,
    },
    KeyLengthExceeded {
        length: u64,
        limit: u64,
    },
    ValueLengthExceeded {
        length: u64,
        limit: u64,
    },
    TotalLogLengthExceeded {
        length: u64,
        limit: u64,
    },
    NumberPromisesExceeded {
        limit: u64,
        number_of_promises: u64,
    },
    NumberInputDataDependenciesExceeded {
        limit: u64,
        number_of_input_data_dependencies: u64,
    },
    ReturnedValueLengthExceeded {
        length: u64,
        limit: u64,
    },
    ContractSizeExceeded {
        limit: u64,
        size: u64,
    },
    Deprecated {
        method_name: String,
    },
    #[serde(rename = "ECRecoverError")]
    EcRecoverError {
        msg: String,
    },
    AltBn128InvalidInput {
        msg: String,
    },
    Ed25519VerifyInvalidInput {
        msg: String,
    },
}

// ============================================================================
// Actions validation errors
// ============================================================================

/// Error validating actions in a transaction or receipt.
#[derive(Debug, Clone, Deserialize)]
pub enum ActionsValidationError {
    DeleteActionMustBeFinal,
    TotalPrepaidGasExceeded {
        limit: Gas,
        total_prepaid_gas: Gas,
    },
    TotalNumberOfActionsExceeded {
        limit: u64,
        total_number_of_actions: u64,
    },
    AddKeyMethodNamesNumberOfBytesExceeded {
        limit: u64,
        total_number_of_bytes: u64,
    },
    AddKeyMethodNameLengthExceeded {
        length: u64,
        limit: u64,
    },
    IntegerOverflow,
    InvalidAccountId {
        account_id: String,
    },
    ContractSizeExceeded {
        limit: u64,
        size: u64,
    },
    FunctionCallMethodNameLengthExceeded {
        length: u64,
        limit: u64,
    },
    FunctionCallArgumentsLengthExceeded {
        length: u64,
        limit: u64,
    },
    UnsuitableStakingKey {
        public_key: PublicKey,
    },
    FunctionCallZeroAttachedGas,
    DelegateActionMustBeOnlyOne,
    UnsupportedProtocolFeature {
        protocol_feature: String,
        version: u32,
    },
    InvalidDeterministicStateInitReceiver {
        derived_id: AccountId,
        receiver_id: AccountId,
    },
    DeterministicStateInitKeyLengthExceeded {
        length: u64,
        limit: u64,
    },
    DeterministicStateInitValueLengthExceeded {
        length: u64,
        limit: u64,
    },
    GasKeyInvalidNumNonces {
        limit: u16,
        requested_nonces: u16,
    },
    AddGasKeyWithNonZeroBalance {
        balance: NearToken,
    },
    GasKeyFunctionCallAllowanceNotAllowed,
}

// ============================================================================
// Receipt validation errors
// ============================================================================

/// Error validating a receipt.
#[derive(Debug, Clone, Deserialize)]
pub enum ReceiptValidationError {
    InvalidPredecessorId {
        account_id: String,
    },
    InvalidReceiverId {
        account_id: String,
    },
    InvalidSignerId {
        account_id: String,
    },
    InvalidDataReceiverId {
        account_id: String,
    },
    ReturnedValueLengthExceeded {
        length: u64,
        limit: u64,
    },
    NumberInputDataDependenciesExceeded {
        limit: u64,
        number_of_input_data_dependencies: u64,
    },
    ActionsValidation(ActionsValidationError),
    ReceiptSizeExceeded {
        limit: u64,
        size: u64,
    },
    InvalidRefundTo {
        account_id: String,
    },
}

// ============================================================================
// Storage errors
// ============================================================================

/// Internal storage error.
#[derive(Debug, Clone, Deserialize)]
pub enum StorageError {
    StorageInternalError,
    MissingTrieValue(MissingTrieValue),
    UnexpectedTrieValue,
    StorageInconsistentState(String),
    FlatStorageBlockNotSupported(String),
    MemTrieLoadingError(String),
}

/// Details about a missing trie value.
#[derive(Debug, Clone, Deserialize)]
pub struct MissingTrieValue {
    pub context: MissingTrieValueContext,
    pub hash: CryptoHash,
}

/// Context in which a trie value was missing.
#[derive(Debug, Clone, Deserialize)]
pub enum MissingTrieValueContext {
    TrieIterator,
    TriePrefetchingStorage,
    TrieMemoryPartialStorage,
    TrieStorage,
}

// ============================================================================
// Misc
// ============================================================================

/// Reason a deposit cost check failed on a gas key transaction.
#[derive(Debug, Clone, Deserialize)]
pub enum DepositCostFailureReason {
    NotEnoughBalance,
    LackBalanceForState,
}
