//! Typed error types for NEAR transaction execution.
//!
//! These types mirror the error hierarchy returned by NEAR RPC in
//! `ExecutionStatus::Failure`, replacing opaque `serde_json::Value`.

use serde::Deserialize;

use super::rpc::GlobalContractIdentifierView;
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
        identifier: GlobalContractIdentifierView,
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
            Self::CreateAccountOnlyByRegistrar { .. } => {
                write!(
                    f,
                    "a top-level account ID can only be created by the registrar"
                )
            }
            Self::CreateAccountNotAllowed {
                account_id,
                predecessor_id,
            } => write!(
                f,
                "account {account_id} must be under a namespace of the creator account {predecessor_id}"
            ),
            Self::ActorNoPermission {
                account_id,
                actor_id,
            } => write!(
                f,
                "actor {actor_id} does not have permission to act on account {account_id}"
            ),
            Self::DeleteKeyDoesNotExist {
                account_id,
                public_key,
            } => write!(
                f,
                "account {account_id} tries to remove an access key {public_key} that doesn't exist"
            ),
            Self::AddKeyAlreadyExists {
                account_id,
                public_key,
            } => write!(
                f,
                "public key {public_key} already exists for account {account_id}"
            ),
            Self::DeleteAccountStaking { account_id } => {
                write!(f, "account {account_id} is staking and cannot be deleted")
            }
            Self::LackBalanceForState { account_id, amount } => write!(
                f,
                "account {account_id} needs {} to cover storage cost",
                amount.exact_amount_display()
            ),
            Self::TriesToUnstake { account_id } => {
                write!(f, "account {account_id} is not staked but tries to unstake")
            }
            Self::TriesToStake {
                account_id,
                stake,
                balance,
                ..
            } => write!(
                f,
                "account {account_id} doesn't have enough balance ({}) to increase the stake ({})",
                balance.exact_amount_display(),
                stake.exact_amount_display()
            ),
            Self::InsufficientStake {
                stake,
                minimum_stake,
                ..
            } => write!(
                f,
                "insufficient stake {}, minimum required is {}",
                stake.exact_amount_display(),
                minimum_stake.exact_amount_display()
            ),
            Self::FunctionCallError(e) => write!(f, "{e}"),
            Self::NewReceiptValidationError(e) => {
                write!(f, "receipt validation error: {e}")
            }
            Self::OnlyImplicitAccountCreationAllowed { account_id } => write!(
                f,
                "only implicit account creation is allowed for {account_id}"
            ),
            Self::DeleteAccountWithLargeState { account_id } => write!(
                f,
                "deleting account {account_id} with large state is temporarily banned"
            ),
            Self::DelegateActionInvalidSignature => {
                write!(f, "invalid signature on delegate action")
            }
            Self::DelegateActionSenderDoesNotMatchTxReceiver {
                sender_id,
                receiver_id,
            } => write!(
                f,
                "delegate action sender {sender_id} does not match transaction receiver {receiver_id}"
            ),
            Self::DelegateActionExpired => write!(f, "delegate action has expired"),
            Self::DelegateActionAccessKeyError(e) => {
                write!(f, "delegate action access key error: {e}")
            }
            Self::DelegateActionInvalidNonce {
                delegate_nonce,
                ak_nonce,
            } => write!(
                f,
                "delegate action nonce {delegate_nonce} must be larger than access key nonce {ak_nonce}"
            ),
            Self::DelegateActionNonceTooLarge {
                delegate_nonce,
                upper_bound,
            } => write!(
                f,
                "delegate action nonce {delegate_nonce} is larger than the upper bound {upper_bound}"
            ),
            Self::GlobalContractDoesNotExist { identifier } => {
                write!(f, "global contract {identifier} does not exist")
            }
            Self::GasKeyDoesNotExist {
                account_id,
                public_key,
            } => write!(
                f,
                "gas key {public_key} does not exist for account {account_id}"
            ),
            Self::InsufficientGasKeyBalance {
                account_id,
                public_key,
                balance,
                ..
            } => write!(
                f,
                "gas key {public_key} for account {account_id} has insufficient balance ({})",
                balance.exact_amount_display()
            ),
            Self::GasKeyBalanceTooHigh {
                account_id,
                public_key,
                balance,
            } => {
                let key_info = match public_key {
                    Some(pk) => format!("gas key {pk}"),
                    None => "gas keys".to_string(),
                };
                write!(
                    f,
                    "balance ({}) of {key_info} for account {account_id} is too high",
                    balance.exact_amount_display()
                )
            }
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
        shard_id: u64,
    },
    ShardStuck {
        missed_chunks: u64,
        shard_id: u64,
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

impl InvalidTxError {
    /// Returns `true` if this error is transient and the transaction may
    /// succeed on retry (e.g. invalid nonce, shard congestion).
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            InvalidTxError::InvalidNonce { .. }
                | InvalidTxError::ShardCongested { .. }
                | InvalidTxError::ShardStuck { .. }
        )
    }
}

impl std::fmt::Display for InvalidTxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidAccessKeyError(e) => write!(f, "{e}"),
            Self::InvalidSignerId { signer_id } => {
                write!(f, "signer ID {signer_id} is not a valid account ID")
            }
            Self::SignerDoesNotExist { signer_id } => {
                write!(f, "signer account {signer_id} does not exist")
            }
            Self::InvalidNonce {
                ak_nonce, tx_nonce, ..
            } => write!(
                f,
                "transaction nonce {tx_nonce} must be larger than access key nonce {ak_nonce}"
            ),
            Self::NonceTooLarge {
                tx_nonce,
                upper_bound,
            } => write!(
                f,
                "transaction nonce {tx_nonce} is larger than the upper bound {upper_bound}"
            ),
            Self::InvalidReceiverId { receiver_id } => {
                write!(f, "receiver ID {receiver_id} is not a valid account ID")
            }
            Self::InvalidSignature => write!(f, "transaction signature is not valid"),
            Self::NotEnoughBalance {
                signer_id,
                balance,
                cost,
            } => write!(
                f,
                "account {signer_id} does not have enough balance ({}) to cover transaction cost ({})",
                balance.exact_amount_display(),
                cost.exact_amount_display()
            ),
            Self::LackBalanceForState { signer_id, amount } => write!(
                f,
                "account {signer_id} doesn't have enough balance ({}) after transaction",
                amount.exact_amount_display()
            ),
            Self::CostOverflow => {
                write!(f, "integer overflow during transaction cost estimation")
            }
            Self::InvalidChain => write!(
                f,
                "transaction parent block hash doesn't belong to the current chain"
            ),
            Self::Expired => write!(f, "transaction has expired"),
            Self::ActionsValidation(e) => write!(f, "{e}"),
            Self::TransactionSizeExceeded { size, limit } => write!(
                f,
                "serialized transaction size ({size}) exceeded the limit ({limit})"
            ),
            Self::InvalidTransactionVersion => write!(f, "invalid transaction version"),
            Self::StorageError(e) => write!(f, "storage error: {e}"),
            Self::ShardCongested {
                shard_id,
                congestion_level,
            } => write!(
                f,
                "shard {shard_id} is too congested ({congestion_level:.2}/1.00) to accept new transactions"
            ),
            Self::ShardStuck {
                shard_id,
                missed_chunks,
            } => write!(
                f,
                "shard {shard_id} is {missed_chunks} blocks behind and can't accept transactions"
            ),
            Self::InvalidNonceIndex {
                tx_nonce_index,
                num_nonces,
            } => write!(
                f,
                "invalid nonce index {tx_nonce_index:?} for key with {num_nonces} nonces"
            ),
            Self::NotEnoughGasKeyBalance {
                signer_id,
                balance,
                cost,
            } => write!(
                f,
                "gas key for {signer_id} does not have enough balance ({}) for gas cost ({})",
                balance.exact_amount_display(),
                cost.exact_amount_display()
            ),
            Self::NotEnoughBalanceForDeposit {
                signer_id,
                balance,
                cost,
                ..
            } => write!(
                f,
                "sender {signer_id} does not have enough balance ({}) to cover deposit cost ({})",
                balance.exact_amount_display(),
                cost.exact_amount_display()
            ),
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

impl std::fmt::Display for InvalidAccessKeyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessKeyNotFound {
                account_id,
                public_key,
            } => write!(
                f,
                "public key {public_key} doesn't exist for account {account_id}"
            ),
            Self::ReceiverMismatch {
                ak_receiver,
                tx_receiver,
            } => write!(
                f,
                "transaction receiver {tx_receiver} doesn't match access key receiver {ak_receiver}"
            ),
            Self::MethodNameMismatch { method_name } => {
                write!(f, "method {method_name} isn't allowed by the access key")
            }
            Self::RequiresFullAccess => {
                write!(f, "transaction requires a full access key")
            }
            Self::NotEnoughAllowance {
                account_id,
                public_key,
                allowance,
                cost,
            } => write!(
                f,
                "access key {public_key} for account {account_id} does not have enough allowance ({}) to cover transaction cost ({})",
                allowance.exact_amount_display(),
                cost.exact_amount_display()
            ),
            Self::DepositWithFunctionCall => {
                write!(f, "deposits are not allowed with function call access keys")
            }
        }
    }
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
            Self::WasmUnknownError => write!(f, "unknown Wasm error"),
            Self::EvmError => write!(f, "EVM error"),
            Self::CompilationError(e) => write!(f, "compilation error: {e}"),
            Self::LinkError { msg } => write!(f, "link error: {msg}"),
            Self::MethodResolveError(e) => write!(f, "method resolve error: {e}"),
            Self::WasmTrap(e) => write!(f, "Wasm trap: {e}"),
            Self::HostError(e) => write!(f, "host error: {e}"),
            Self::ExecutionError(msg) => write!(f, "execution error: {msg}"),
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

impl std::fmt::Display for CompilationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CodeDoesNotExist { account_id } => {
                write!(f, "contract code does not exist for account {account_id}")
            }
            Self::PrepareError(e) => write!(f, "{e}"),
            Self::WasmerCompileError { msg } => write!(f, "Wasmer compilation error: {msg}"),
        }
    }
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

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Serialization => write!(f, "Wasm serialization error"),
            Self::Deserialization => write!(f, "Wasm deserialization error"),
            Self::InternalMemoryDeclared => {
                write!(f, "Wasm module declares internal memory")
            }
            Self::GasInstrumentation => write!(f, "gas instrumentation failed"),
            Self::StackHeightInstrumentation => {
                write!(f, "stack height instrumentation failed")
            }
            Self::Instantiate => write!(f, "Wasm instantiation error"),
            Self::Memory => write!(f, "Wasm memory error"),
            Self::TooManyFunctions => write!(f, "too many functions in Wasm module"),
            Self::TooManyLocals => write!(f, "too many locals in Wasm module"),
            Self::TooManyTables => write!(f, "too many tables in Wasm module"),
            Self::TooManyTableElements => {
                write!(f, "too many table elements in Wasm module")
            }
        }
    }
}

/// Error resolving a method in Wasm.
#[derive(Debug, Clone, Deserialize)]
pub enum MethodResolveError {
    MethodEmptyName,
    MethodNotFound,
    MethodInvalidSignature,
}

impl std::fmt::Display for MethodResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MethodEmptyName => write!(f, "method name is empty"),
            Self::MethodNotFound => write!(f, "method not found in contract"),
            Self::MethodInvalidSignature => write!(f, "method has an invalid signature"),
        }
    }
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

impl std::fmt::Display for WasmTrap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "unreachable instruction executed"),
            Self::IncorrectCallIndirectSignature => {
                write!(f, "incorrect call indirect signature")
            }
            Self::MemoryOutOfBounds => write!(f, "memory out of bounds"),
            Self::CallIndirectOob => write!(f, "call indirect out of bounds"),
            Self::IllegalArithmetic => write!(f, "illegal arithmetic operation"),
            Self::MisalignedAtomicAccess => write!(f, "misaligned atomic access"),
            Self::IndirectCallToNull => write!(f, "indirect call to null"),
            Self::StackOverflow => write!(f, "stack overflow"),
            Self::GenericTrap => write!(f, "generic trap"),
        }
    }
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

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadUtf16 => write!(f, "bad UTF-16 string"),
            Self::BadUtf8 => write!(f, "bad UTF-8 string"),
            Self::GasExceeded => write!(f, "gas exceeded"),
            Self::GasLimitExceeded => write!(f, "gas limit exceeded"),
            Self::BalanceExceeded => write!(f, "balance exceeded"),
            Self::EmptyMethodName => write!(f, "method name is empty"),
            Self::GuestPanic { panic_msg } => write!(f, "smart contract panicked: {panic_msg}"),
            Self::IntegerOverflow => write!(f, "integer overflow"),
            Self::InvalidPromiseIndex { promise_idx } => {
                write!(f, "invalid promise index {promise_idx}")
            }
            Self::CannotAppendActionToJointPromise => {
                write!(f, "cannot append action to joint promise")
            }
            Self::CannotReturnJointPromise => write!(f, "cannot return joint promise"),
            Self::InvalidPromiseResultIndex { result_idx } => {
                write!(f, "invalid promise result index {result_idx}")
            }
            Self::InvalidRegisterId { register_id } => {
                write!(f, "invalid register ID {register_id}")
            }
            Self::IteratorWasInvalidated { iterator_index } => {
                write!(f, "iterator {iterator_index} was invalidated")
            }
            Self::MemoryAccessViolation => write!(f, "memory access violation"),
            Self::InvalidReceiptIndex { receipt_index } => {
                write!(f, "invalid receipt index {receipt_index}")
            }
            Self::InvalidIteratorIndex { iterator_index } => {
                write!(f, "invalid iterator index {iterator_index}")
            }
            Self::InvalidAccountId => write!(f, "invalid account ID"),
            Self::InvalidMethodName => write!(f, "invalid method name"),
            Self::InvalidPublicKey => write!(f, "invalid public key"),
            Self::ProhibitedInView { method_name } => {
                write!(f, "method {method_name} is not allowed in a view call")
            }
            Self::NumberOfLogsExceeded { limit } => {
                write!(f, "number of logs exceeded the limit of {limit}")
            }
            Self::KeyLengthExceeded { length, limit } => {
                write!(f, "key length {length} exceeded the limit of {limit}")
            }
            Self::ValueLengthExceeded { length, limit } => {
                write!(f, "value length {length} exceeded the limit of {limit}")
            }
            Self::TotalLogLengthExceeded { length, limit } => {
                write!(f, "total log length {length} exceeded the limit of {limit}")
            }
            Self::NumberPromisesExceeded {
                number_of_promises,
                limit,
            } => write!(
                f,
                "number of promises {number_of_promises} exceeded the limit of {limit}"
            ),
            Self::NumberInputDataDependenciesExceeded {
                number_of_input_data_dependencies,
                limit,
            } => write!(
                f,
                "number of input data dependencies {number_of_input_data_dependencies} exceeded the limit of {limit}"
            ),
            Self::ReturnedValueLengthExceeded { length, limit } => {
                write!(
                    f,
                    "returned value length {length} exceeded the limit of {limit}"
                )
            }
            Self::ContractSizeExceeded { size, limit } => {
                write!(f, "contract size {size} exceeded the limit of {limit}")
            }
            Self::Deprecated { method_name } => {
                write!(f, "method {method_name} is deprecated")
            }
            Self::EcRecoverError { msg } => write!(f, "EC recover error: {msg}"),
            Self::AltBn128InvalidInput { msg } => {
                write!(f, "AltBn128 invalid input: {msg}")
            }
            Self::Ed25519VerifyInvalidInput { msg } => {
                write!(f, "Ed25519 verification invalid input: {msg}")
            }
        }
    }
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

impl std::fmt::Display for ActionsValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DeleteActionMustBeFinal => {
                write!(f, "delete action must be the final action in a transaction")
            }
            Self::TotalPrepaidGasExceeded {
                total_prepaid_gas,
                limit,
            } => write!(
                f,
                "total prepaid gas ({total_prepaid_gas}) exceeded the limit ({limit})"
            ),
            Self::TotalNumberOfActionsExceeded {
                total_number_of_actions,
                limit,
            } => write!(
                f,
                "number of actions ({total_number_of_actions}) exceeded the limit ({limit})"
            ),
            Self::AddKeyMethodNamesNumberOfBytesExceeded {
                total_number_of_bytes,
                limit,
            } => write!(
                f,
                "total size of method names ({total_number_of_bytes} bytes) exceeded the limit ({limit}) in add key action"
            ),
            Self::AddKeyMethodNameLengthExceeded { length, limit } => write!(
                f,
                "method name length ({length}) exceeded the limit ({limit}) in add key action"
            ),
            Self::IntegerOverflow => write!(f, "integer overflow"),
            Self::InvalidAccountId { account_id } => {
                write!(f, "invalid account ID {account_id}")
            }
            Self::ContractSizeExceeded { size, limit } => write!(
                f,
                "contract size ({size}) exceeded the limit ({limit}) in deploy action"
            ),
            Self::FunctionCallMethodNameLengthExceeded { length, limit } => write!(
                f,
                "method name length ({length}) exceeded the limit ({limit}) in function call action"
            ),
            Self::FunctionCallArgumentsLengthExceeded { length, limit } => write!(
                f,
                "arguments length ({length}) exceeded the limit ({limit}) in function call action"
            ),
            Self::UnsuitableStakingKey { public_key } => {
                write!(f, "public key {public_key} is not suitable for staking")
            }
            Self::FunctionCallZeroAttachedGas => {
                write!(
                    f,
                    "function call must have a positive amount of gas attached"
                )
            }
            Self::DelegateActionMustBeOnlyOne => {
                write!(f, "transaction must contain only one delegate action")
            }
            Self::UnsupportedProtocolFeature {
                protocol_feature,
                version,
            } => write!(
                f,
                "protocol feature {protocol_feature} is unsupported in version {version}"
            ),
            Self::InvalidDeterministicStateInitReceiver {
                derived_id,
                receiver_id,
            } => write!(
                f,
                "invalid receiver {receiver_id} for deterministic account {derived_id}"
            ),
            Self::DeterministicStateInitKeyLengthExceeded { length, limit } => write!(
                f,
                "deterministic state init key length ({length}) exceeded the limit ({limit})"
            ),
            Self::DeterministicStateInitValueLengthExceeded { length, limit } => write!(
                f,
                "deterministic state init value length ({length}) exceeded the limit ({limit})"
            ),
            Self::GasKeyInvalidNumNonces {
                requested_nonces,
                limit,
            } => write!(
                f,
                "gas key requested invalid number of nonces: {requested_nonces} (must be between 1 and {limit})"
            ),
            Self::AddGasKeyWithNonZeroBalance { balance } => write!(
                f,
                "adding a gas key with non-zero balance ({}) is not allowed",
                balance.exact_amount_display()
            ),
            Self::GasKeyFunctionCallAllowanceNotAllowed => write!(
                f,
                "gas keys with function call permission cannot have an allowance"
            ),
        }
    }
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

impl std::fmt::Display for ReceiptValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPredecessorId { account_id } => {
                write!(f, "invalid predecessor ID {account_id}")
            }
            Self::InvalidReceiverId { account_id } => {
                write!(f, "invalid receiver ID {account_id}")
            }
            Self::InvalidSignerId { account_id } => {
                write!(f, "invalid signer ID {account_id}")
            }
            Self::InvalidDataReceiverId { account_id } => {
                write!(f, "invalid data receiver ID {account_id}")
            }
            Self::ReturnedValueLengthExceeded { length, limit } => write!(
                f,
                "returned value length ({length}) exceeded the limit ({limit})"
            ),
            Self::NumberInputDataDependenciesExceeded {
                number_of_input_data_dependencies,
                limit,
            } => write!(
                f,
                "number of input data dependencies ({number_of_input_data_dependencies}) exceeded the limit ({limit})"
            ),
            Self::ActionsValidation(e) => write!(f, "{e}"),
            Self::ReceiptSizeExceeded { size, limit } => {
                write!(f, "receipt size ({size}) exceeded the limit ({limit})")
            }
            Self::InvalidRefundTo { account_id } => {
                write!(f, "invalid refund-to account ID {account_id}")
            }
        }
    }
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
#[allow(clippy::enum_variant_names)] // Matches nearcore naming
pub enum MissingTrieValueContext {
    TrieIterator,
    TriePrefetchingStorage,
    TrieMemoryPartialStorage,
    TrieStorage,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageInternalError => write!(f, "internal storage error"),
            Self::MissingTrieValue(v) => {
                write!(f, "missing trie value with hash {}", v.hash)
            }
            Self::UnexpectedTrieValue => write!(f, "unexpected trie value"),
            Self::StorageInconsistentState(msg) => {
                write!(f, "storage is in an inconsistent state: {msg}")
            }
            Self::FlatStorageBlockNotSupported(msg) => {
                write!(f, "block is not supported by flat storage: {msg}")
            }
            Self::MemTrieLoadingError(msg) => {
                write!(f, "trie is not loaded in memory: {msg}")
            }
        }
    }
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

impl std::fmt::Display for DepositCostFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotEnoughBalance => write!(f, "not enough balance"),
            Self::LackBalanceForState => write!(f, "not enough balance for state"),
        }
    }
}
