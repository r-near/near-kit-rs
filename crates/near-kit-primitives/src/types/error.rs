//! Typed error types for NEAR transaction execution.
//!
//! These types mirror nearcore's error types, variant for variant and field
//! for field, as returned by NEAR RPC in `ExecutionStatus::Failure`.

// nearcore's docs describe each variant; the docs here cover what near-kit
// adds and variants whose meaning isn't obvious from the name.
#![allow(missing_docs)]

use serde::Deserialize;

use super::rpc::GlobalContractIdentifierView;
use super::{AccountId, CryptoHash, Gas, NearToken, PublicKey};

/// Raw JSON payload of an error this version of near-kit could not parse
/// into a typed variant.
///
/// Emits a `tracing::warn!` when constructed during deserialization (with the
/// `tracing` feature enabled) so fallback misclassification is observable.
#[derive(Debug, Clone)]
pub struct UnknownError(pub serde_json::Value);

impl<'de> serde::Deserialize<'de> for UnknownError {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        crate::trace::warn!(raw = %value, "unrecognized error variant from RPC, falling back to Unknown");
        Ok(Self(value))
    }
}

impl std::fmt::Display for UnknownError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Error returned by NEAR RPC when a transaction or receipt fails.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum TxExecutionError {
    #[error("ActionError: {0}")]
    ActionError(ActionError),
    #[error("InvalidTxError: {0}")]
    InvalidTxError(InvalidTxError),
}

/// An error that occurred during action execution.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionError {
    /// Index of the failed action in the transaction.
    /// Not defined if kind is `ActionErrorKind::LackBalanceForState`.
    #[serde(default)]
    pub index: Option<u64>,
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
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum ActionErrorKind {
    #[error("account {account_id} already exists")]
    AccountAlreadyExists { account_id: AccountId },
    #[error("account {account_id} does not exist")]
    AccountDoesNotExist { account_id: AccountId },
    #[error(
        "top-level account {account_id} can only be created by registrar {registrar_account_id}, not by {predecessor_id}"
    )]
    CreateAccountOnlyByRegistrar {
        account_id: AccountId,
        predecessor_id: AccountId,
        registrar_account_id: AccountId,
    },
    #[error(
        "account {account_id} must be under a namespace of the creator account {predecessor_id}"
    )]
    CreateAccountNotAllowed {
        account_id: AccountId,
        predecessor_id: AccountId,
    },
    #[error("actor {actor_id} does not have permission to act on account {account_id}")]
    ActorNoPermission {
        account_id: AccountId,
        actor_id: AccountId,
    },
    #[error("account {account_id} tries to remove an access key {public_key} that doesn't exist")]
    DeleteKeyDoesNotExist {
        account_id: AccountId,
        public_key: PublicKey,
    },
    #[error("public key {public_key} already exists for account {account_id}")]
    AddKeyAlreadyExists {
        account_id: AccountId,
        public_key: PublicKey,
    },
    #[error("account {account_id} is staking and cannot be deleted")]
    DeleteAccountStaking { account_id: AccountId },
    #[error(
        "account {account_id} needs {} to cover storage cost",
        .amount.exact_amount_display()
    )]
    LackBalanceForState {
        account_id: AccountId,
        amount: NearToken,
    },
    #[error("account {account_id} is not staked but tries to unstake")]
    TriesToUnstake { account_id: AccountId },
    #[error(
        "account {account_id} doesn't have enough balance ({}) to increase the stake ({})",
        .balance.exact_amount_display(),
        .stake.exact_amount_display()
    )]
    TriesToStake {
        account_id: AccountId,
        balance: NearToken,
        locked: NearToken,
        stake: NearToken,
    },
    #[error(
        "account {account_id} has insufficient stake ({}), minimum required is {}",
        .stake.exact_amount_display(),
        .minimum_stake.exact_amount_display()
    )]
    InsufficientStake {
        account_id: AccountId,
        minimum_stake: NearToken,
        stake: NearToken,
    },
    #[error("{0}")]
    FunctionCallError(FunctionCallError),
    #[error("receipt validation error: {0}")]
    NewReceiptValidationError(ReceiptValidationError),
    #[error("only implicit account creation is allowed for {account_id}")]
    OnlyImplicitAccountCreationAllowed { account_id: AccountId },
    #[error("deleting account {account_id} with large state is temporarily banned")]
    DeleteAccountWithLargeState { account_id: AccountId },
    #[error("invalid signature on delegate action")]
    DelegateActionInvalidSignature,
    #[error("delegate action sender {sender_id} does not match transaction receiver {receiver_id}")]
    DelegateActionSenderDoesNotMatchTxReceiver {
        receiver_id: AccountId,
        sender_id: AccountId,
    },
    #[error("delegate action has expired")]
    DelegateActionExpired,
    #[error("delegate action access key error: {0}")]
    DelegateActionAccessKeyError(InvalidAccessKeyError),
    #[error(
        "delegate action nonce {delegate_nonce} must be larger than access key nonce {ak_nonce}"
    )]
    DelegateActionInvalidNonce { ak_nonce: u64, delegate_nonce: u64 },
    #[error("delegate action nonce {delegate_nonce} is larger than the upper bound {upper_bound}")]
    DelegateActionNonceTooLarge {
        delegate_nonce: u64,
        upper_bound: u64,
    },
    #[error("global contract {identifier} does not exist")]
    GlobalContractDoesNotExist {
        identifier: GlobalContractIdentifierView,
    },
    #[error("gas key {public_key} does not exist for account {account_id}")]
    GasKeyDoesNotExist {
        account_id: AccountId,
        public_key: PublicKey,
    },
    #[error(
        "gas key {public_key} for account {account_id} has insufficient balance ({}, required: {})",
        .balance.exact_amount_display(),
        .required.exact_amount_display()
    )]
    InsufficientGasKeyBalance {
        account_id: AccountId,
        balance: NearToken,
        public_key: PublicKey,
        required: NearToken,
    },
    #[error(
        "balance ({}) of {} for account {account_id} is too high",
        .balance.exact_amount_display(),
        gas_key_label(.public_key.as_ref())
    )]
    GasKeyBalanceTooHigh {
        account_id: AccountId,
        balance: NearToken,
        #[serde(default)]
        public_key: Option<PublicKey>,
    },
    #[error(
        "delegate action nonce index {nonce_index} must be smaller than the gas key nonce count {num_nonces}"
    )]
    DelegateActionInvalidNonceIndex { nonce_index: u16, num_nonces: u16 },
    #[error("combined size of the receipt's promise inputs ({size}) exceeded the limit ({limit})")]
    TotalPromiseInputSizeExceeded { size: u64, limit: u64 },
    #[error("receipt exceeded the storage proof size limit of {limit} bytes")]
    ReceiptStorageProofSizeExceeded { limit: u64 },
    #[error("universal state init payload is not a canonical state init")]
    MalformedUniversalStateInit,
    #[error("account {account_id} has no state init applied yet")]
    AccountNotInitialized { account_id: AccountId },
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown action error: {0}")]
    Unknown(UnknownError),
}

fn gas_key_label(public_key: Option<&PublicKey>) -> String {
    match public_key {
        Some(pk) => format!("gas key {pk}"),
        None => "gas keys".to_string(),
    }
}

/// An error during transaction validation (before execution).
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum InvalidTxError {
    #[error("{0}")]
    InvalidAccessKeyError(InvalidAccessKeyError),
    #[error("signer ID {signer_id} is not a valid account ID")]
    InvalidSignerId { signer_id: String },
    #[error("signer account {signer_id} does not exist")]
    SignerDoesNotExist { signer_id: AccountId },
    #[error("transaction nonce {tx_nonce} must be larger than access key nonce {ak_nonce}")]
    InvalidNonce { ak_nonce: u64, tx_nonce: u64 },
    #[error("transaction nonce {tx_nonce} is larger than the upper bound {upper_bound}")]
    NonceTooLarge { tx_nonce: u64, upper_bound: u64 },
    #[error("receiver ID {receiver_id} is not a valid account ID")]
    InvalidReceiverId { receiver_id: String },
    #[error("transaction signature is not valid")]
    InvalidSignature,
    #[error(
        "account {signer_id} does not have enough balance ({}) to cover transaction cost ({})",
        .balance.exact_amount_display(),
        .cost.exact_amount_display()
    )]
    NotEnoughBalance {
        balance: NearToken,
        cost: NearToken,
        signer_id: AccountId,
    },
    #[error(
        "account {signer_id} doesn't have enough balance ({}) after transaction",
        .amount.exact_amount_display()
    )]
    LackBalanceForState {
        amount: NearToken,
        signer_id: AccountId,
    },
    #[error("integer overflow during transaction cost estimation")]
    CostOverflow,
    #[error("transaction parent block hash doesn't belong to the current chain")]
    InvalidChain,
    #[error("transaction has expired")]
    Expired,
    #[error("{0}")]
    ActionsValidation(ActionsValidationError),
    #[error("serialized transaction size ({size}) exceeded the limit ({limit})")]
    TransactionSizeExceeded { limit: u64, size: u64 },
    #[error("invalid transaction version")]
    InvalidTransactionVersion,
    #[error("storage error: {0}")]
    StorageError(StorageError),
    #[error(
        "shard {shard_id} is too congested ({congestion_level:.2}/1.00) to accept new transactions"
    )]
    ShardCongested {
        congestion_level: f64,
        shard_id: u64,
    },
    #[error("shard {shard_id} is {missed_chunks} blocks behind and can't accept transactions")]
    ShardStuck { missed_chunks: u64, shard_id: u64 },
    #[error("{}", nonce_index_message(*.tx_nonce_index, *.num_nonces))]
    InvalidNonceIndex {
        num_nonces: u16,
        #[serde(default)]
        tx_nonce_index: Option<u16>,
    },
    #[error(
        "gas key for {signer_id} does not have enough balance ({}) for gas cost ({})",
        .balance.exact_amount_display(),
        .cost.exact_amount_display()
    )]
    NotEnoughGasKeyBalance {
        balance: NearToken,
        cost: NearToken,
        signer_id: AccountId,
    },
    #[error(
        "signer {signer_id} does not have enough balance ({}) to cover deposit cost ({}): {reason}",
        .balance.exact_amount_display(),
        .cost.exact_amount_display()
    )]
    NotEnoughBalanceForDeposit {
        balance: NearToken,
        cost: NearToken,
        reason: DepositCostFailureReason,
        signer_id: AccountId,
    },
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown transaction error: {0}")]
    Unknown(UnknownError),
}

fn nonce_index_message(tx_nonce_index: Option<u16>, num_nonces: u16) -> String {
    match tx_nonce_index {
        Some(idx) => format!("invalid nonce index {idx} for key with {num_nonces} nonces"),
        None => format!("missing nonce index for key with {num_nonces} nonces"),
    }
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

/// Error related to access key validation.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum InvalidAccessKeyError {
    #[error("public key {public_key} doesn't exist for account {account_id}")]
    AccessKeyNotFound {
        account_id: AccountId,
        public_key: PublicKey,
    },
    #[error("transaction receiver {tx_receiver} doesn't match access key receiver {ak_receiver}")]
    ReceiverMismatch {
        ak_receiver: String,
        tx_receiver: AccountId,
    },
    #[error("method {method_name} isn't allowed by the access key")]
    MethodNameMismatch { method_name: String },
    #[error("transaction requires a full access key")]
    RequiresFullAccess,
    #[error(
        "access key {public_key} for account {account_id} does not have enough allowance ({}) to cover transaction cost ({})",
        .allowance.exact_amount_display(),
        .cost.exact_amount_display()
    )]
    NotEnoughAllowance {
        account_id: AccountId,
        allowance: NearToken,
        cost: NearToken,
        public_key: PublicKey,
    },
    #[error("deposits are not allowed with function call access keys")]
    DepositWithFunctionCall,
    #[error("gas keys can't sign a delegate action with a plain nonce")]
    DelegateActionRequiresNonGasKey,
    #[error("a delegate action with a gas key nonce must be signed by a gas key")]
    DelegateActionRequiresGasKey,
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown access key error: {0}")]
    Unknown(UnknownError),
}

/// An error during function call execution.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum FunctionCallError {
    #[error("unknown Wasm error")]
    WasmUnknownError,
    #[serde(rename = "_EVMError")]
    #[error("EVM error")]
    EvmError,
    #[error("compilation error: {0}")]
    CompilationError(CompilationError),
    #[error("link error: {msg}")]
    LinkError { msg: String },
    #[error("method resolve error: {0}")]
    MethodResolveError(MethodResolveError),
    #[error("Wasm trap: {0}")]
    WasmTrap(WasmTrap),
    #[error("host error: {0}")]
    HostError(HostError),
    #[error("execution error: {0}")]
    ExecutionError(String),
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown function call error: {0}")]
    Unknown(UnknownError),
}

/// Wasm compilation error.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum CompilationError {
    #[error("contract code does not exist for account {account_id}")]
    CodeDoesNotExist { account_id: AccountId },
    #[error("{0}")]
    PrepareError(PrepareError),
    #[error("Wasmer compilation error: {msg}")]
    WasmerCompileError { msg: String },
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown compilation error: {0}")]
    Unknown(UnknownError),
}

/// Error preparing a Wasm module.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum PrepareError {
    #[error("Wasm serialization error")]
    Serialization,
    #[error("Wasm deserialization error")]
    Deserialization,
    #[error("Wasm module declares internal memory")]
    InternalMemoryDeclared,
    #[error("gas instrumentation failed")]
    GasInstrumentation,
    #[error("stack height instrumentation failed")]
    StackHeightInstrumentation,
    #[error("Wasm instantiation error")]
    Instantiate,
    #[error("Wasm memory error")]
    Memory,
    #[error("too many functions in Wasm module")]
    TooManyFunctions,
    #[error("too many locals in Wasm module")]
    TooManyLocals,
    #[error("too many tables in Wasm module")]
    TooManyTables,
    #[error("too many table elements in Wasm module")]
    TooManyTableElements,
    #[error("a function body in the Wasm module is too large")]
    FunctionBodyTooLarge,
    #[error("instrumented Wasm code exceeds the size limit")]
    InstrumentedCodeTooLarge,
    #[error("too many basic blocks in a Wasm function")]
    TooManyBlocksPerFunction,
    #[error("too many basic blocks in Wasm module")]
    TooManyBlocksPerContract,
    #[error("too many types in Wasm module")]
    TooManyTypes,
    #[error("too many parameters in a Wasm function")]
    TooManyParamsPerFunction,
    #[error("too many parameters in Wasm module")]
    TooManyParamsPerContract,
    #[error("operand stack of a Wasm function is too large")]
    OperandStackTooLarge,
    #[error("too many globals in Wasm module")]
    TooManyGlobals,
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown prepare error: {0}")]
    Unknown(UnknownError),
}

/// Error resolving a method in Wasm.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum MethodResolveError {
    #[error("method name is empty")]
    MethodEmptyName,
    #[error("method not found in contract")]
    MethodNotFound,
    #[error("method has an invalid signature")]
    MethodInvalidSignature,
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown method resolve error: {0}")]
    Unknown(UnknownError),
}

/// A trap during Wasm execution.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum WasmTrap {
    #[error("unreachable instruction executed")]
    Unreachable,
    #[error("incorrect call indirect signature")]
    IncorrectCallIndirectSignature,
    #[error("memory out of bounds")]
    MemoryOutOfBounds,
    #[serde(rename = "CallIndirectOOB")]
    #[error("call indirect out of bounds")]
    CallIndirectOob,
    #[error("illegal arithmetic operation")]
    IllegalArithmetic,
    #[error("misaligned atomic access")]
    MisalignedAtomicAccess,
    #[error("indirect call to null")]
    IndirectCallToNull,
    #[error("stack overflow")]
    StackOverflow,
    #[error("generic trap")]
    GenericTrap,
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown Wasm trap: {0}")]
    Unknown(UnknownError),
}

/// Error from a host function call.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum HostError {
    #[serde(rename = "BadUTF16")]
    #[error("bad UTF-16 string")]
    BadUtf16,
    #[serde(rename = "BadUTF8")]
    #[error("bad UTF-8 string")]
    BadUtf8,
    #[error("gas exceeded")]
    GasExceeded,
    #[error("gas limit exceeded")]
    GasLimitExceeded,
    #[error("balance exceeded")]
    BalanceExceeded,
    #[error("method name is empty")]
    EmptyMethodName,
    #[error("smart contract panicked: {panic_msg}")]
    GuestPanic { panic_msg: String },
    #[error("integer overflow")]
    IntegerOverflow,
    #[error("invalid promise index {promise_idx}")]
    InvalidPromiseIndex { promise_idx: u64 },
    #[error("cannot append action to joint promise")]
    CannotAppendActionToJointPromise,
    #[error("cannot return joint promise")]
    CannotReturnJointPromise,
    #[error("invalid promise result index {result_idx}")]
    InvalidPromiseResultIndex { result_idx: u64 },
    #[error("invalid register ID {register_id}")]
    InvalidRegisterId { register_id: u64 },
    #[error("iterator {iterator_index} was invalidated")]
    IteratorWasInvalidated { iterator_index: u64 },
    #[error("memory access violation")]
    MemoryAccessViolation,
    #[error("invalid receipt index {receipt_index}")]
    InvalidReceiptIndex { receipt_index: u64 },
    #[error("invalid iterator index {iterator_index}")]
    InvalidIteratorIndex { iterator_index: u64 },
    #[error("invalid account ID")]
    InvalidAccountId,
    #[error("invalid method name")]
    InvalidMethodName,
    #[error("invalid public key")]
    InvalidPublicKey,
    #[error("method {method_name} is not allowed in a view call")]
    ProhibitedInView { method_name: String },
    #[error("number of logs exceeded the limit of {limit}")]
    NumberOfLogsExceeded { limit: u64 },
    #[error("key length {length} exceeded the limit of {limit}")]
    KeyLengthExceeded { length: u64, limit: u64 },
    #[error("value length {length} exceeded the limit of {limit}")]
    ValueLengthExceeded { length: u64, limit: u64 },
    #[error("total log length {length} exceeded the limit of {limit}")]
    TotalLogLengthExceeded { length: u64, limit: u64 },
    #[error("number of promises {number_of_promises} exceeded the limit of {limit}")]
    NumberPromisesExceeded { limit: u64, number_of_promises: u64 },
    #[error(
        "number of input data dependencies {number_of_input_data_dependencies} exceeded the limit of {limit}"
    )]
    NumberInputDataDependenciesExceeded {
        limit: u64,
        number_of_input_data_dependencies: u64,
    },
    #[error("returned value length {length} exceeded the limit of {limit}")]
    ReturnedValueLengthExceeded { length: u64, limit: u64 },
    #[error("contract size {size} exceeded the limit of {limit}")]
    ContractSizeExceeded { limit: u64, size: u64 },
    #[error("method {method_name} is deprecated")]
    Deprecated { method_name: String },
    #[serde(rename = "ECRecoverError")]
    #[error("EC recover error: {msg}")]
    EcRecoverError { msg: String },
    #[error("AltBn128 invalid input: {msg}")]
    AltBn128InvalidInput { msg: String },
    #[error("Ed25519 verification invalid input: {msg}")]
    Ed25519VerifyInvalidInput { msg: String },
    #[error("P-256 verification invalid input: {msg}")]
    P256VerifyInvalidInput { msg: String },
    #[error("ML-DSA verification invalid input: {msg}")]
    MlDsaVerifyInvalidInput { msg: String },
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown host error: {0}")]
    Unknown(UnknownError),
}

/// Error validating actions in a transaction or receipt.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum ActionsValidationError {
    #[error("delete action must be the final action in a transaction")]
    DeleteActionMustBeFinal,
    #[error("total prepaid gas ({total_prepaid_gas}) exceeded the limit ({limit})")]
    TotalPrepaidGasExceeded { limit: Gas, total_prepaid_gas: Gas },
    #[error("number of actions ({total_number_of_actions}) exceeded the limit ({limit})")]
    TotalNumberOfActionsExceeded {
        limit: u64,
        total_number_of_actions: u64,
    },
    #[error(
        "total size of method names ({total_number_of_bytes} bytes) exceeded the limit ({limit}) in add key action"
    )]
    AddKeyMethodNamesNumberOfBytesExceeded {
        limit: u64,
        total_number_of_bytes: u64,
    },
    #[error("method name length ({length}) exceeded the limit ({limit}) in add key action")]
    AddKeyMethodNameLengthExceeded { length: u64, limit: u64 },
    #[error("integer overflow")]
    IntegerOverflow,
    #[error("invalid account ID {account_id}")]
    InvalidAccountId { account_id: String },
    #[error("contract size ({size}) exceeded the limit ({limit}) in deploy action")]
    ContractSizeExceeded { limit: u64, size: u64 },
    #[error("method name length ({length}) exceeded the limit ({limit}) in function call action")]
    FunctionCallMethodNameLengthExceeded { length: u64, limit: u64 },
    #[error("arguments length ({length}) exceeded the limit ({limit}) in function call action")]
    FunctionCallArgumentsLengthExceeded { length: u64, limit: u64 },
    #[error("public key {public_key} is not suitable for staking")]
    UnsuitableStakingKey { public_key: PublicKey },
    #[error("function call must have a positive amount of gas attached")]
    FunctionCallZeroAttachedGas,
    #[error("transaction must contain only one delegate action")]
    DelegateActionMustBeOnlyOne,
    #[error("protocol feature {protocol_feature} is unsupported in version {version}")]
    UnsupportedProtocolFeature {
        protocol_feature: String,
        version: u32,
    },
    #[error("invalid receiver {receiver_id} for deterministic account {derived_id}")]
    InvalidDeterministicStateInitReceiver {
        derived_id: AccountId,
        receiver_id: AccountId,
    },
    #[error("deterministic state init key length ({length}) exceeded the limit ({limit})")]
    DeterministicStateInitKeyLengthExceeded { length: u64, limit: u64 },
    #[error("deterministic state init value length ({length}) exceeded the limit ({limit})")]
    DeterministicStateInitValueLengthExceeded { length: u64, limit: u64 },
    #[error(
        "gas key requested invalid number of nonces: {requested_nonces} (must be between 1 and {limit})"
    )]
    GasKeyInvalidNumNonces { limit: u16, requested_nonces: u16 },
    #[error(
        "adding a gas key with non-zero balance ({}) is not allowed",
        .balance.exact_amount_display()
    )]
    AddGasKeyWithNonZeroBalance { balance: NearToken },
    #[error("gas keys with function call permission cannot have an allowance")]
    GasKeyFunctionCallAllowanceNotAllowed,
    #[error("number of deploy actions ({number_of_deploy_actions}) exceeded the limit ({limit})")]
    TotalNumberOfDeployActionsExceeded {
        limit: u64,
        number_of_deploy_actions: u64,
    },
    #[error("method name in a function call action must not be empty")]
    FunctionCallEmptyMethodName,
    #[error("invalid receiver {receiver_id} for universal account {derived_id}")]
    InvalidUniversalStateInitReceiver {
        derived_id: AccountId,
        receiver_id: AccountId,
    },
    #[error("universal state init key length ({length}) exceeded the limit ({limit})")]
    UniversalStateInitKeyLengthExceeded { length: u64, limit: u64 },
    #[error("universal state init value length ({length}) exceeded the limit ({limit})")]
    UniversalStateInitValueLengthExceeded { length: u64, limit: u64 },
    #[error("universal state init bytes are not a canonical borsh encoding of a state init")]
    MalformedUniversalStateInit,
    #[error(
        "protocol feature {protocol_feature} was removed and is not supported in version {version}"
    )]
    RemovedProtocolFeature {
        protocol_feature: String,
        version: u32,
    },
    #[error("withdraw from gas key actions are not allowed inside a delegate action")]
    WithdrawFromGasKeyNotAllowedInDelegate,
    #[error(
        "state inits in this receipt commit to {number_of_keys} access keys, exceeding the limit ({limit})"
    )]
    TotalNumberOfStateInitKeysExceeded { limit: u64, number_of_keys: u64 },
    #[error(
        "state inits in this receipt carry {number_of_entries} storage entries, exceeding the limit ({limit})"
    )]
    TotalNumberOfStateInitEntriesExceeded { limit: u64, number_of_entries: u64 },
    #[error("transfers to the universal account {account_id} are not allowed yet")]
    TransferToUniversalAccountNotAllowed { account_id: AccountId },
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown actions validation error: {0}")]
    Unknown(UnknownError),
}

/// Error validating a receipt.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum ReceiptValidationError {
    #[error("invalid predecessor ID {account_id}")]
    InvalidPredecessorId { account_id: String },
    #[error("invalid receiver ID {account_id}")]
    InvalidReceiverId { account_id: String },
    #[error("invalid signer ID {account_id}")]
    InvalidSignerId { account_id: String },
    #[error("invalid data receiver ID {account_id}")]
    InvalidDataReceiverId { account_id: String },
    #[error("returned value length ({length}) exceeded the limit ({limit})")]
    ReturnedValueLengthExceeded { length: u64, limit: u64 },
    #[error(
        "number of input data dependencies ({number_of_input_data_dependencies}) exceeded the limit ({limit})"
    )]
    NumberInputDataDependenciesExceeded {
        limit: u64,
        number_of_input_data_dependencies: u64,
    },
    #[error("{0}")]
    ActionsValidation(ActionsValidationError),
    #[error("receipt size ({size}) exceeded the limit ({limit})")]
    ReceiptSizeExceeded { limit: u64, size: u64 },
    #[error("invalid refund-to account ID {account_id}")]
    InvalidRefundTo { account_id: String },
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown receipt validation error: {0}")]
    Unknown(UnknownError),
}

/// Internal storage error.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum StorageError {
    #[error("internal storage error")]
    StorageInternalError,
    #[error("missing trie value {} in {}", .0.hash, .0.context)]
    MissingTrieValue(MissingTrieValue),
    #[error("unexpected trie value")]
    UnexpectedTrieValue,
    #[error("storage is in an inconsistent state: {0}")]
    StorageInconsistentState(String),
    #[error("block is not supported by flat storage: {0}")]
    FlatStorageBlockNotSupported(String),
    #[error("trie is not loaded in memory: {0}")]
    MemTrieLoadingError(String),
    /// Unrecognized variant or field shape; holds the raw JSON.
    #[serde(untagged)]
    #[error("unknown storage error: {0}")]
    Unknown(UnknownError),
}

/// Details about a missing trie value.
#[derive(Debug, Clone, Deserialize)]
pub struct MissingTrieValue {
    pub context: MissingTrieValueContext,
    pub hash: CryptoHash,
}

/// Context in which a trie value was missing.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
#[allow(clippy::enum_variant_names)] // Matches nearcore naming
pub enum MissingTrieValueContext {
    #[error("trie iterator")]
    TrieIterator,
    #[error("prefetching storage")]
    TriePrefetchingStorage,
    #[error("memory partial storage")]
    TrieMemoryPartialStorage,
    #[error("trie storage")]
    TrieStorage,
}

/// Reason a deposit cost check failed on a gas key transaction.
#[derive(Debug, Clone, Deserialize, thiserror::Error)]
pub enum DepositCostFailureReason {
    #[error("not enough balance")]
    NotEnoughBalance,
    #[error("not enough balance for state")]
    LackBalanceForState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepare_error_2_12_variants() {
        for (json, expected) in [
            ("\"FunctionBodyTooLarge\"", "function body"),
            ("\"InstrumentedCodeTooLarge\"", "instrumented"),
            ("\"TooManyBlocksPerFunction\"", "basic blocks"),
            ("\"TooManyBlocksPerContract\"", "basic blocks"),
            ("\"TooManyTypes\"", "types"),
            ("\"TooManyParamsPerFunction\"", "parameters"),
            ("\"TooManyParamsPerContract\"", "parameters"),
            ("\"OperandStackTooLarge\"", "operand stack"),
            ("\"TooManyGlobals\"", "globals"),
        ] {
            let err: PrepareError = serde_json::from_str(json).unwrap();
            assert!(
                !matches!(err, PrepareError::Unknown(_)),
                "{json} fell through to Unknown"
            );
            assert!(err.to_string().contains(expected), "{json}: {err}");
        }
    }

    #[test]
    fn test_host_error_p256_verify_invalid_input() {
        let json = serde_json::json!({
            "P256VerifyInvalidInput": { "msg": "signature must be 64 bytes" }
        });
        let err: HostError = serde_json::from_value(json).unwrap();
        match &err {
            HostError::P256VerifyInvalidInput { msg } => {
                assert_eq!(msg, "signature must be 64 bytes");
            }
            other => panic!("Expected P256VerifyInvalidInput, got {other:?}"),
        }
        assert!(err.to_string().contains("P-256"));
    }

    #[test]
    fn test_actions_validation_total_deploy_actions_exceeded() {
        let json = serde_json::json!({
            "TotalNumberOfDeployActionsExceeded": {
                "number_of_deploy_actions": 5,
                "limit": 4
            }
        });
        let err: ActionsValidationError = serde_json::from_value(json).unwrap();
        match &err {
            ActionsValidationError::TotalNumberOfDeployActionsExceeded {
                number_of_deploy_actions,
                limit,
            } => {
                assert_eq!(*number_of_deploy_actions, 5);
                assert_eq!(*limit, 4);
            }
            other => panic!("Expected TotalNumberOfDeployActionsExceeded, got {other:?}"),
        }
    }

    const UNIVERSAL_ACCOUNT_ID: &str = "0u000g40r40m30e209185gr38e1w8124gk2gahc5rr34d1p70x3rfg";

    #[test]
    fn test_actions_validation_2_14_variants() {
        for (json, expected) in [
            (
                serde_json::json!("FunctionCallEmptyMethodName"),
                "must not be empty",
            ),
            (
                serde_json::json!({
                    "InvalidUniversalStateInitReceiver": {
                        "receiver_id": "alice.near",
                        "derived_id": UNIVERSAL_ACCOUNT_ID
                    }
                }),
                "invalid receiver alice.near",
            ),
            (
                serde_json::json!({
                    "UniversalStateInitKeyLengthExceeded": { "length": 3000, "limit": 2048 }
                }),
                "key length (3000)",
            ),
            (
                serde_json::json!({
                    "UniversalStateInitValueLengthExceeded": { "length": 5000, "limit": 4096 }
                }),
                "value length (5000)",
            ),
            (
                serde_json::json!("MalformedUniversalStateInit"),
                "canonical borsh",
            ),
            (
                serde_json::json!({
                    "RemovedProtocolFeature": {
                        "protocol_feature": "DelegateV2",
                        "version": 87
                    }
                }),
                "DelegateV2 was removed",
            ),
            (
                serde_json::json!("WithdrawFromGasKeyNotAllowedInDelegate"),
                "inside a delegate action",
            ),
            (
                serde_json::json!({
                    "TotalNumberOfStateInitKeysExceeded": { "number_of_keys": 9, "limit": 8 }
                }),
                "9 access keys",
            ),
            (
                serde_json::json!({
                    "TotalNumberOfStateInitEntriesExceeded": {
                        "number_of_entries": 300,
                        "limit": 256
                    }
                }),
                "300 storage entries",
            ),
            (
                serde_json::json!({
                    "TransferToUniversalAccountNotAllowed": { "account_id": UNIVERSAL_ACCOUNT_ID }
                }),
                UNIVERSAL_ACCOUNT_ID,
            ),
        ] {
            let err: ActionsValidationError = serde_json::from_value(json.clone()).unwrap();
            assert!(
                !matches!(err, ActionsValidationError::Unknown(_)),
                "{json} fell through to Unknown"
            );
            assert!(err.to_string().contains(expected), "{json}: {err}");
        }
    }

    #[test]
    fn test_action_error_kind_2_14_variants() {
        for (json, expected) in [
            (
                serde_json::json!({
                    "DelegateActionInvalidNonceIndex": { "nonce_index": 4, "num_nonces": 4 }
                }),
                "nonce index 4",
            ),
            (
                serde_json::json!({
                    "TotalPromiseInputSizeExceeded": { "size": 5000, "limit": 4096 }
                }),
                "promise inputs (5000)",
            ),
            (
                serde_json::json!({ "ReceiptStorageProofSizeExceeded": { "limit": 4000000 } }),
                "4000000 bytes",
            ),
            (
                serde_json::json!("MalformedUniversalStateInit"),
                "not a canonical state init",
            ),
            (
                serde_json::json!({
                    "AccountNotInitialized": { "account_id": UNIVERSAL_ACCOUNT_ID }
                }),
                "no state init applied",
            ),
        ] {
            let err: ActionErrorKind = serde_json::from_value(json.clone()).unwrap();
            assert!(
                !matches!(err, ActionErrorKind::Unknown(_)),
                "{json} fell through to Unknown"
            );
            assert!(err.to_string().contains(expected), "{json}: {err}");
        }
    }

    #[test]
    fn test_invalid_access_key_2_14_variants() {
        for (json, expected) in [
            ("\"DelegateActionRequiresNonGasKey\"", "plain nonce"),
            ("\"DelegateActionRequiresGasKey\"", "signed by a gas key"),
        ] {
            let err: InvalidAccessKeyError = serde_json::from_str(json).unwrap();
            assert!(
                !matches!(err, InvalidAccessKeyError::Unknown(_)),
                "{json} fell through to Unknown"
            );
            assert!(err.to_string().contains(expected), "{json}: {err}");
        }
    }

    #[test]
    fn test_host_error_ml_dsa_verify_invalid_input() {
        let json = serde_json::json!({
            "MlDsaVerifyInvalidInput": { "msg": "signature must be 3309 bytes" }
        });
        let err: HostError = serde_json::from_value(json).unwrap();
        match &err {
            HostError::MlDsaVerifyInvalidInput { msg } => {
                assert_eq!(msg, "signature must be 3309 bytes");
            }
            other => panic!("Expected MlDsaVerifyInvalidInput, got {other:?}"),
        }
        assert!(err.to_string().contains("ML-DSA"));
    }

    #[test]
    fn test_invalid_tx_transfer_to_universal_account_not_allowed() {
        // A transaction transferring to a `0u` account is rejected at submission.
        let json = serde_json::json!({
            "InvalidTxError": {
                "ActionsValidation": {
                    "TransferToUniversalAccountNotAllowed": { "account_id": UNIVERSAL_ACCOUNT_ID }
                }
            }
        });
        let err: TxExecutionError = serde_json::from_value(json).unwrap();
        match &err {
            TxExecutionError::InvalidTxError(InvalidTxError::ActionsValidation(
                ActionsValidationError::TransferToUniversalAccountNotAllowed { account_id },
            )) => assert_eq!(account_id.as_str(), UNIVERSAL_ACCOUNT_ID),
            other => panic!("Expected InvalidTxError::ActionsValidation, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_struct_variant_falls_back() {
        let json = serde_json::json!({
            "SomeFutureError": { "limit": 1, "actual": 2 }
        });
        let err: ActionErrorKind = serde_json::from_value(json).unwrap();
        assert!(matches!(err, ActionErrorKind::Unknown(_)));
        assert!(err.to_string().contains("SomeFutureError"));
    }

    #[test]
    fn test_known_tag_with_incompatible_shape_falls_back_to_unknown() {
        // A known tag whose payload no longer matches the expected field
        // shape (here a required field is missing) is captured by the
        // untagged `Unknown` fallback rather than failing the whole parse.
        // The raw JSON is preserved so callers can still inspect it. This is
        // the same forward-compat behavior we want when nearcore changes a
        // known variant's field shape between releases.
        let json = serde_json::json!({ "AccountAlreadyExists": {} });
        let err: ActionErrorKind = serde_json::from_value(json.clone()).unwrap();
        match err {
            ActionErrorKind::Unknown(raw) => assert_eq!(raw.0, json),
            other => panic!("Expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_unknown_unit_variant_falls_back() {
        let err: HostError = serde_json::from_str("\"SomeFutureHostError\"").unwrap();
        assert!(matches!(err, HostError::Unknown(_)));

        let err: PrepareError = serde_json::from_str("\"SomeFuturePrepareError\"").unwrap();
        assert!(matches!(err, PrepareError::Unknown(_)));

        let err: WasmTrap = serde_json::from_str("\"SomeFutureTrap\"").unwrap();
        assert!(matches!(err, WasmTrap::Unknown(_)));
    }

    #[test]
    fn test_known_variants_not_shadowed_by_fallback() {
        let err: PrepareError = serde_json::from_str("\"GasInstrumentation\"").unwrap();
        assert!(matches!(err, PrepareError::GasInstrumentation));

        let json = serde_json::json!({ "GuestPanic": { "panic_msg": "boom" } });
        let err: HostError = serde_json::from_value(json).unwrap();
        assert!(matches!(err, HostError::GuestPanic { .. }));
    }

    #[test]
    fn test_unknown_nested_variant_preserves_outer_structure() {
        // A failed receipt whose kind comes from a future nearcore version
        // must still deserialize as an ActionError instead of failing the
        // whole response parse.
        let json = serde_json::json!({
            "ActionError": {
                "index": 0,
                "kind": { "BrandNewErrorKind": { "foo": "bar" } }
            }
        });
        let err: TxExecutionError = serde_json::from_value(json).unwrap();
        match err {
            TxExecutionError::ActionError(action_error) => {
                assert_eq!(action_error.index, Some(0));
                assert!(matches!(action_error.kind, ActionErrorKind::Unknown(_)));
            }
            other => panic!("Expected ActionError, got {other:?}"),
        }
    }
}
