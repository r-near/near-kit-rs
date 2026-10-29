//! RPC response types.

use std::collections::BTreeMap;
use std::ops::Deref;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_with::{DisplayFromStr, base64::Base64, serde_as};

use crate::error::OutcomeError;

use super::action::{
    AccessKey, AccessKeyPermission, Action, AddKeyAction, CreateAccountAction, DelegateAction,
    DelegateActionV2, DeleteAccountAction, DeleteKeyAction, DeterministicStateInitAction,
    FunctionCallAction, FunctionCallPermission, GasKeyInfo, GlobalContractId, NonDelegateAction,
    SignedDelegateAction, StakeAction, StateInit, StateInitV1, TransferAction,
    TransferToGasKeyAction, UniversalStateInitAction, UseGlobalContractAction,
    VersionedDelegateActionPayload, VersionedSignedDelegateAction, WithdrawFromGasKeyAction,
};
use super::block_reference::TxExecutionStatus;
use super::error::{ActionError, InvalidTxError, TxExecutionError};
use super::transaction::{NonceMode, TransactionNonce};
use super::{AccountId, CryptoHash, Gas, NearToken, Nonce, PublicKey, PublicKeyHandle, Signature};
use crate::error::ActionViewConversionError;

/// Cost per byte of storage in yoctoNEAR (0.00001 NEAR), a protocol constant
/// used for the available balance. See
/// <https://docs.near.org/concepts/storage/storage-staking>.
pub const STORAGE_AMOUNT_PER_BYTE: u128 = 10_000_000_000_000_000_000;

/// Account information from view_account RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountView {
    /// Total balance including locked.
    pub amount: NearToken,
    /// Locked balance (staked).
    pub locked: NearToken,
    /// Hash of deployed contract code (or zeros if none).
    pub code_hash: CryptoHash,
    /// Storage used in bytes.
    pub storage_usage: u64,
    /// Storage paid at block height (deprecated, always 0).
    #[serde(default)]
    pub storage_paid_at: u64,
    /// Global contract code hash (if using a global contract).
    pub global_contract_hash: Option<CryptoHash>,
    /// Global contract account ID (if using a global contract by account).
    pub global_contract_account_id: Option<AccountId>,
    /// Whether the account's state is installed (see [`AccountState`]).
    #[serde(default)]
    pub state: AccountState,
    /// Last used nonce of an uninitialized universal account (`None` once
    /// initialized). The state init transaction must use `bootstrap_nonce + 1`.
    pub bootstrap_nonce: Option<u64>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// Whether an account's state is installed (nearcore 2.14). Only `0u`
/// universal accounts can be [`Uninitialized`](Self::Uninitialized): funded,
/// but with no keys, code, or data until `UniversalStateInit` is applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountState {
    /// The account's state is installed.
    #[default]
    Initialized,
    /// A universal account whose state init hasn't been applied yet.
    Uninitialized,
}

impl AccountView {
    /// Get available (spendable) balance: `amount` minus
    /// [`storage_cost`](Self::storage_cost). Staked tokens count towards the
    /// storage requirement, so if `locked` covers it all of `amount` is
    /// available.
    pub fn available(&self) -> NearToken {
        self.amount.saturating_sub(self.storage_cost())
    }

    /// Get the amount of NEAR reserved for storage costs:
    /// `max(0, storage_usage * STORAGE_AMOUNT_PER_BYTE - locked)`.
    pub fn storage_cost(&self) -> NearToken {
        let required = STORAGE_AMOUNT_PER_BYTE.saturating_mul(u128::from(self.storage_usage));
        NearToken::from_yoctonear(required).saturating_sub(self.locked)
    }

    /// Check if this account has a deployed contract.
    pub fn has_contract(&self) -> bool {
        !self.code_hash.is_zero()
    }
}

/// Simplified balance info.
#[derive(Debug, Clone)]
pub struct AccountBalance {
    /// Total balance (available + locked).
    pub total: NearToken,
    /// Available balance (spendable, accounting for storage).
    pub available: NearToken,
    /// Locked balance (staked).
    pub locked: NearToken,
    /// Amount reserved for storage costs.
    pub storage_cost: NearToken,
    /// Storage used in bytes.
    pub storage_usage: u64,
}

impl std::fmt::Display for AccountBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.available)
    }
}

impl From<AccountView> for AccountBalance {
    fn from(view: AccountView) -> Self {
        Self {
            total: view.amount,
            available: view.available(),
            locked: view.locked,
            storage_cost: view.storage_cost(),
            storage_usage: view.storage_usage,
        }
    }
}

/// Access key information from view_access_key RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyView {
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Permission level.
    pub permission: AccessKeyPermissionView,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// Access key details (without block info, used in lists).
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyDetails {
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Permission level.
    pub permission: AccessKeyPermissionView,
}

/// Access key permission from RPC.
#[derive(Debug, Clone, Deserialize)]
pub enum AccessKeyPermissionView {
    /// Full access.
    FullAccess,
    /// Function call access with restrictions.
    FunctionCall {
        /// Maximum amount this key can spend.
        allowance: Option<NearToken>,
        /// Contract that can be called.
        receiver_id: AccountId,
        /// Methods that can be called (empty = all).
        method_names: Vec<String>,
    },
    /// Gas key with function call access.
    GasKeyFunctionCall {
        /// Gas key balance.
        balance: NearToken,
        /// Number of nonces.
        num_nonces: u16,
        /// Maximum amount this key can spend.
        allowance: Option<NearToken>,
        /// Contract that can be called.
        receiver_id: AccountId,
        /// Methods that can be called (empty = all).
        method_names: Vec<String>,
    },
    /// Gas key with full access.
    GasKeyFullAccess {
        /// Gas key balance.
        balance: NearToken,
        /// Number of nonces.
        num_nonces: u16,
    },
}

/// Access key list from view_access_key_list RPC.
///
/// One page of an account's access keys. The node caps how many keys a single
/// response carries (100 by default), so a listing can be truncated:
/// [`last_key`](Self::last_key) is then the continuation cursor. The fetch-all
/// helper (`RpcClient::view_access_key_list`) follows that
/// cursor for you and always returns `last_key: None`; only
/// `RpcClient::view_access_key_list_page` hands it back for manual paging.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyListView {
    /// List of access keys.
    pub keys: Vec<AccessKeyInfoView>,
    /// Pagination cursor. `Some` means this page was truncated: pass it as
    /// `after_key` to fetch the keys after it. `None` means this was the last
    /// page (or the full list).
    pub last_key: Option<PublicKeyHandle>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// Single access key info in list.
#[derive(Debug, Clone, Deserialize)]
pub struct AccessKeyInfoView {
    /// How the chain identifies the key: the full public key for
    /// ed25519/secp256k1, or the `ml-dsa-65-hash:` digest for ML-DSA-65 (the
    /// full key is not stored on-chain). Use
    /// [`as_full()`](PublicKeyHandle::as_full) to get a usable
    /// [`PublicKey`], or [`refers_to()`](PublicKeyHandle::refers_to) to check
    /// it against one.
    pub public_key: PublicKeyHandle,
    /// Access key details.
    pub access_key: AccessKeyDetails,
}

/// Parallel nonces assigned to a gas key by `view_gas_key_nonces`.
#[derive(Debug, Clone, Deserialize)]
pub struct GasKeyNoncesView {
    /// Nonces indexed in the same order as the gas key's nonce slots.
    pub nonces: Vec<Nonce>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// A single contract state entry (key/value), as returned by `view_state`.
///
/// `key` and `value` are raw bytes; they are base64-encoded on the wire.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StateItem {
    /// Raw state key bytes.
    #[serde_as(as = "Base64")]
    pub key: Vec<u8>,
    /// Raw state value bytes.
    #[serde_as(as = "Base64")]
    pub value: Vec<u8>,
}

/// Result of a `view_state` query (one page).
///
/// When the trie holds more entries than `limit`, [`ViewStateResult::last_key`]
/// is the continuation cursor: pass it back as `after_key` to fetch the next
/// page. It is `None` on the last page.
///
/// Like every `query` view, it carries the block the page was read at.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ViewStateResult {
    /// The contract state entries in this page.
    pub values: Vec<StateItem>,
    /// Continuation cursor (raw key bytes). Absent on the last page.
    #[serde_as(as = "Option<Base64>")]
    #[serde(default)]
    pub last_key: Option<Vec<u8>>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// A contract's full state as collected by `view_state_all`.
///
/// `values` holds every matching [`StateItem`] across all pages. Like every
/// `query` view it carries the block the state was read at: `block_height` and
/// `block_hash` come from the first page, and every subsequent page was
/// requested against that same `block_hash`, so the whole result is a
/// consistent snapshot of one block.
///
/// The type derefs to `[StateItem]` and iterates over its entries, so
/// `result.len()`, `result.iter()`, `&result[0]` and `for item in result` all
/// work directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewStateAllResult {
    /// Every contract state entry matching the prefix, in trie order.
    pub values: Vec<StateItem>,
    /// Block height the state was read at (taken from the first page).
    pub block_height: u64,
    /// Block hash the state was read at (taken from the first page); every
    /// page was read against this hash.
    pub block_hash: CryptoHash,
}

impl Deref for ViewStateAllResult {
    type Target = [StateItem];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl IntoIterator for ViewStateAllResult {
    type Item = StateItem;
    type IntoIter = std::vec::IntoIter<StateItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl<'a> IntoIterator for &'a ViewStateAllResult {
    type Item = &'a StateItem;
    type IntoIter = std::slice::Iter<'a, StateItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

/// A contract's WASM code and its hash, as returned by `view_code` and
/// `view_global_contract_code` queries.
///
/// `code` is raw WASM bytes; it is base64-encoded on the wire. `hash` is the
/// SHA-256 hash of the code. Like every `query` view, it carries the block the
/// code was read at.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ContractCodeView {
    /// Raw WASM bytes of the contract.
    #[serde(rename = "code_base64")]
    #[serde_as(as = "Base64")]
    pub code: Vec<u8>,
    /// SHA-256 hash of the code.
    pub hash: CryptoHash,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

/// One entry in a [`BlockEffects`] response: which kind of state was touched
/// for an account in the block.
///
/// `block_effects` reports only the *kind* of change per account, with no
/// cause or value.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum StateChangeKindView {
    /// The account itself (balance, etc.) was touched.
    AccountTouched {
        /// The account that was touched.
        account_id: AccountId,
    },
    /// One of the account's access keys was touched.
    AccessKeyTouched {
        /// The account that was touched.
        account_id: AccountId,
    },
    /// The account's contract data (storage) was touched.
    DataTouched {
        /// The account that was touched.
        account_id: AccountId,
    },
    /// The account's contract code was touched.
    ContractCodeTouched {
        /// The account that was touched.
        account_id: AccountId,
    },
}

/// All state changes within a single block, from the `block_effects` RPC
/// (stabilized name for `EXPERIMENTAL_changes_in_block`).
#[derive(Debug, Clone, Deserialize)]
pub struct BlockEffects {
    /// Hash of the block these changes belong to.
    pub block_hash: CryptoHash,
    /// The kinds of state change applied in this block (one entry per touched
    /// account/state-kind).
    pub changes: Vec<StateChangeKindView>,
}

/// A single maintenance window — a half-open block-height range
/// `[start, end)` during which a validator has no chunk/block production
/// duties. Returned by the `maintenance_windows` RPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct MaintenanceWindow {
    /// First block height of the window (inclusive).
    pub start: u64,
    /// Block height the window ends at (exclusive).
    pub end: u64,
}

/// Block information from block RPC.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockView {
    /// Block author (validator account ID).
    pub author: AccountId,
    /// Block header.
    pub header: BlockHeaderView,
    /// List of chunks in the block.
    pub chunks: Vec<ChunkHeaderView>,
}

/// Block header with full details.
#[derive(Debug, Clone, Deserialize)]
pub struct BlockHeaderView {
    /// Block height.
    pub height: u64,
    /// Previous block height (may be None for genesis).
    pub prev_height: Option<u64>,
    /// Block hash.
    pub hash: CryptoHash,
    /// Previous block hash.
    pub prev_hash: CryptoHash,
    /// Previous state root.
    pub prev_state_root: CryptoHash,
    /// Chunk receipts root.
    pub chunk_receipts_root: CryptoHash,
    /// Chunk headers root.
    pub chunk_headers_root: CryptoHash,
    /// Chunk transaction root.
    pub chunk_tx_root: CryptoHash,
    /// Outcome root.
    pub outcome_root: CryptoHash,
    /// Number of chunks included.
    pub chunks_included: u64,
    /// Challenges root.
    pub challenges_root: CryptoHash,
    /// Timestamp in nanoseconds (as u64).
    pub timestamp: u64,
    /// Timestamp in nanoseconds (as string for precision).
    pub timestamp_nanosec: String,
    /// Random value for the block.
    pub random_value: CryptoHash,
    /// Validator proposals.
    #[serde(default)]
    pub validator_proposals: Vec<ValidatorStakeView>,
    /// Chunk mask (which shards have chunks).
    #[serde(default)]
    pub chunk_mask: Vec<bool>,
    /// Gas price for this block.
    pub gas_price: NearToken,
    /// Block ordinal (may be None).
    pub block_ordinal: Option<u64>,
    /// Total supply of NEAR tokens.
    pub total_supply: NearToken,
    /// Challenges result.
    #[serde(default)]
    pub challenges_result: Vec<SlashedValidator>,
    /// Last final block hash.
    pub last_final_block: CryptoHash,
    /// Last DS final block hash.
    pub last_ds_final_block: CryptoHash,
    /// Epoch ID.
    pub epoch_id: CryptoHash,
    /// Next epoch ID.
    pub next_epoch_id: CryptoHash,
    /// Next block producer hash.
    pub next_bp_hash: CryptoHash,
    /// Block merkle root.
    pub block_merkle_root: CryptoHash,
    /// Epoch sync data hash (optional).
    pub epoch_sync_data_hash: Option<CryptoHash>,
    /// Block body hash (optional, added in later protocol versions).
    pub block_body_hash: Option<CryptoHash>,
    /// Block approvals (nullable signatures).
    #[serde(default)]
    pub approvals: Vec<Option<Signature>>,
    /// Block signature.
    pub signature: Signature,
    /// Latest protocol version.
    pub latest_protocol_version: u32,
    /// Rent paid (deprecated; when present, always 0).
    pub rent_paid: Option<NearToken>,
    /// Validator reward (deprecated; when present, always 0).
    pub validator_reward: Option<NearToken>,
    /// Chunk endorsements (optional).
    pub chunk_endorsements: Option<Vec<Vec<u8>>>,
    /// Shard split info (optional).
    pub shard_split: Option<(u64, AccountId)>,
}

/// Validator stake, as listed in validator proposals in block and chunk
/// headers.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorStakeView {
    /// Validator account ID.
    pub account_id: AccountId,
    /// Public key.
    pub public_key: PublicKey,
    /// Stake amount.
    pub stake: NearToken,
}

/// Slashed validator from challenge results.
#[derive(Debug, Clone, Deserialize)]
pub struct SlashedValidator {
    /// Validator account ID.
    pub account_id: AccountId,
    /// Whether this was a double sign.
    pub is_double_sign: bool,
}

/// Chunk header with full details.
#[derive(Debug, Clone, Deserialize)]
pub struct ChunkHeaderView {
    /// Chunk hash.
    pub chunk_hash: CryptoHash,
    /// Previous block hash.
    pub prev_block_hash: CryptoHash,
    /// Outcome root.
    pub outcome_root: CryptoHash,
    /// Previous state root.
    pub prev_state_root: CryptoHash,
    /// Encoded merkle root.
    pub encoded_merkle_root: CryptoHash,
    /// Encoded length.
    pub encoded_length: u64,
    /// Height when chunk was created.
    pub height_created: u64,
    /// Height when chunk was included.
    pub height_included: u64,
    /// Shard ID.
    pub shard_id: u64,
    /// Gas used in this chunk.
    pub gas_used: u64,
    /// Gas limit for this chunk.
    pub gas_limit: u64,
    /// Validator reward.
    pub validator_reward: NearToken,
    /// Balance burnt.
    pub balance_burnt: NearToken,
    /// Outgoing receipts root.
    pub outgoing_receipts_root: CryptoHash,
    /// Transaction root.
    pub tx_root: CryptoHash,
    /// Validator proposals.
    #[serde(default)]
    pub validator_proposals: Vec<ValidatorStakeView>,
    /// Congestion info (optional, added in later protocol versions).
    pub congestion_info: Option<CongestionInfoView>,
    /// Bandwidth requests (optional, added in later protocol versions).
    pub bandwidth_requests: Option<BandwidthRequests>,
    /// Rent paid (deprecated; when present, always 0).
    pub rent_paid: Option<NearToken>,
    /// Proposed trie split for resharding (`None` when absent or `null`).
    pub proposed_split: Option<TrieSplit>,
    /// Chunk signature.
    pub signature: Signature,
}

/// Bandwidth requests for a chunk (versioned).
#[derive(Debug, Clone, Deserialize)]
pub enum BandwidthRequests {
    /// Version 1.
    V1(BandwidthRequestsV1),
}

/// Bandwidth requests data (V1).
#[derive(Debug, Clone, Deserialize)]
pub struct BandwidthRequestsV1 {
    /// List of bandwidth requests.
    pub requests: Vec<BandwidthRequest>,
}

/// A single bandwidth request to a target shard.
#[derive(Debug, Clone, Deserialize)]
pub struct BandwidthRequest {
    /// Target shard index.
    pub to_shard: u16,
    /// Bitmap of requested values.
    pub requested_values_bitmap: BandwidthRequestBitmap,
}

/// Bitmap for bandwidth request values.
#[derive(Debug, Clone, Deserialize)]
pub struct BandwidthRequestBitmap {
    /// Raw bitmap data.
    pub data: [u8; 5],
}

/// Trie split information for resharding.
#[derive(Debug, Clone, Deserialize)]
pub struct TrieSplit {
    /// Account boundary for the split.
    pub boundary_account: AccountId,
    /// Memory usage of the left child.
    pub left_memory: u64,
    /// Memory usage of the right child.
    pub right_memory: u64,
}

/// Congestion information for a shard.
#[serde_as]
#[derive(Debug, Clone, Deserialize)]
pub struct CongestionInfoView {
    /// Gas used by delayed receipts.
    #[serde_as(as = "DisplayFromStr")]
    #[serde(default)]
    pub delayed_receipts_gas: u128,
    /// Gas used by buffered receipts.
    #[serde_as(as = "DisplayFromStr")]
    #[serde(default)]
    pub buffered_receipts_gas: u128,
    /// Bytes used by receipts.
    #[serde(default)]
    pub receipt_bytes: u64,
    /// Allowed shard.
    #[serde(default)]
    pub allowed_shard: u16,
}

/// Gas price response.
#[derive(Debug, Clone, Deserialize)]
pub struct GasPrice {
    /// Gas price in yoctoNEAR.
    pub gas_price: NearToken,
}

/// Overall transaction execution status.
///
/// Represents the final result of a transaction, matching nearcore's `FinalExecutionStatus`.
/// This is the `status` field in `FinalExecutionOutcome`.
#[derive(Debug, Clone, Deserialize)]
pub enum FinalExecutionStatus {
    /// The transaction has not yet started execution.
    NotStarted,
    /// The transaction has started but the first receipt hasn't completed.
    Started,
    /// The transaction execution failed.
    Failure(TxExecutionError),
    /// The transaction execution succeeded (base64-encoded return value).
    SuccessValue(String),
}

/// Response returned for non-executed wait levels
/// ([`Submitted`](crate::transaction::Submitted),
/// [`Included`](crate::transaction::Included),
/// [`IncludedFinal`](crate::transaction::IncludedFinal)): enough to poll for
/// the final result later via `Near::tx_status`.
#[derive(Debug, Clone)]
pub struct SendTxResponse {
    /// Hash of the submitted transaction.
    pub transaction_hash: CryptoHash,
    /// Account ID of the transaction signer.
    pub sender_id: AccountId,
    /// The execution outcome, once the node has one, exposed as-is (not
    /// validated): partial while the transaction runs, complete once it has
    /// finished. Check [`FinalExecutionOutcome::status`] for how far it got.
    ///
    /// - [`Submitted`](crate::transaction::Submitted): a sent transaction gets
    ///   none; `tx_status` returns whatever the node already has.
    /// - [`Included`](crate::transaction::Included): often [`None`], as the
    ///   node answers before it has stored the outcome.
    /// - [`IncludedFinal`](crate::transaction::IncludedFinal): present.
    ///
    /// Its `receipts` are filled in only by `Near::tx_status`
    /// (`EXPERIMENTAL_tx_status`), which lets a frontend poll at an early wait
    /// level to drive a progressive UI; from a send (`send_tx`) only
    /// `receipts_outcome` is.
    pub outcome: Option<FinalExecutionOutcome>,
}

/// Response from `EXPERIMENTAL_receipt_to_tx`: the transaction that produced a receipt.
#[derive(Debug, Clone, Deserialize)]
pub struct ReceiptToTxResponse {
    /// Hash of the transaction that produced the receipt.
    pub transaction_hash: CryptoHash,
    /// Account that signed the transaction.
    pub sender_account_id: AccountId,
}

/// A `send_tx` or `EXPERIMENTAL_tx_status` response: the wait level the
/// transaction reached and, once it has executed, its outcome.
///
/// near-kit's low-level `RpcClient::send_tx` and `RpcClient::tx_status`
/// return it. Sending through a client instead resolves to the typed
/// response of its [`WaitLevel`](crate::transaction::WaitLevel)
/// ([`FinalExecutionOutcome`] or [`SendTxResponse`]), built from this.
///
/// `outcome` follows the rules of [`SendTxResponse::outcome`].
///
/// Deserializing fails if the JSON carries an outcome that does not decode
/// (or only some of its fields); `outcome` is `None` only when the JSON has
/// no outcome fields at all.
#[derive(Debug, Clone)]
pub struct RpcTransactionResponse {
    /// Hash of the transaction. Not part of the JSON: `send_tx` fills it in
    /// from the signed transaction and `tx_status` from the outcome or the
    /// requested hash, so it is all zeros when you deserialize the JSON
    /// yourself.
    pub transaction_hash: CryptoHash,
    /// The wait level that was reached.
    pub final_execution_status: TxExecutionStatus,
    /// The execution outcome, present when the transaction has been executed.
    pub outcome: Option<FinalExecutionOutcome>,
}

// nearcore flattens an `Option<FinalExecutionOutcomeViewEnum>` into the
// response. A derived `#[serde(flatten)] Option<_>` would turn any decode
// error inside the outcome into `None`, so decode the outcome fields
// individually and require all or none of them.
impl<'de> Deserialize<'de> for RpcTransactionResponse {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            final_execution_status: TxExecutionStatus,
            status: Option<FinalExecutionStatus>,
            transaction: Option<TransactionView>,
            transaction_outcome: Option<ExecutionOutcomeWithId>,
            receipts_outcome: Option<Vec<ExecutionOutcomeWithId>>,
            receipts: Option<Vec<Receipt>>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let outcome = match (
            wire.status,
            wire.transaction,
            wire.transaction_outcome,
            wire.receipts_outcome,
        ) {
            (
                Some(status),
                Some(transaction),
                Some(transaction_outcome),
                Some(receipts_outcome),
            ) => Some(FinalExecutionOutcome {
                status,
                transaction,
                transaction_outcome,
                receipts_outcome,
                receipts: wire.receipts.unwrap_or_default(),
            }),
            (None, None, None, None) if wire.receipts.is_none() => None,
            (status, transaction, transaction_outcome, receipts_outcome) => {
                let missing = [
                    ("status", status.is_none()),
                    ("transaction", transaction.is_none()),
                    ("transaction_outcome", transaction_outcome.is_none()),
                    ("receipts_outcome", receipts_outcome.is_none()),
                ]
                .into_iter()
                .find_map(|(field, missing)| missing.then_some(field))
                .unwrap_or("status");
                return Err(serde::de::Error::missing_field(missing));
            }
        };
        Ok(Self {
            transaction_hash: CryptoHash::default(),
            final_execution_status: wire.final_execution_status,
            outcome,
        })
    }
}

/// Final execution outcome from a NEAR transaction.
///
/// Returned by both `send_tx` and `EXPERIMENTAL_tx_status` once the
/// transaction has executed.
///
/// A failed action still arrives as `Ok(outcome)`, so the type is
/// `#[must_use]`: check [`is_failure`](Self::is_failure) or
/// [`receipt_failures`](Self::receipt_failures), or call
/// [`result`](Self::result) to turn the failure into an `Err`.
#[derive(Debug, Clone, Deserialize)]
#[must_use = "a failed action is still Ok(outcome): check is_failure(), receipt_failures() or call result()"]
pub struct FinalExecutionOutcome {
    /// Overall transaction execution result.
    pub status: FinalExecutionStatus,
    /// The transaction that was executed.
    pub transaction: TransactionView,
    /// Outcome of the transaction itself.
    pub transaction_outcome: ExecutionOutcomeWithId,
    /// Outcomes of all receipts spawned by the transaction.
    pub receipts_outcome: Vec<ExecutionOutcomeWithId>,
    /// Full receipt details (populated by `tx_status`, empty from `send`).
    #[serde(default)]
    pub receipts: Vec<Receipt>,
}

/// Dropping a [`FinalExecutionOutcome`] is a `unused_must_use` warning:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// fn send() -> near_kit_primitives::rpc::FinalExecutionOutcome {
///     unimplemented!()
/// }
/// fn main() {
///     send();
/// }
/// ```
///
/// Binding it compiles:
///
/// ```no_run
/// #![deny(unused_must_use)]
/// fn send() -> near_kit_primitives::rpc::FinalExecutionOutcome {
///     unimplemented!()
/// }
/// fn main() {
///     let _outcome = send();
/// }
/// ```
#[cfg(doctest)]
struct FinalExecutionOutcomeMustUse;

impl FinalExecutionOutcome {
    /// Whether the transaction's top-level [`status`](Self::status) is a
    /// success.
    ///
    /// This reads the main chain of receipts only. A receipt that fails off
    /// that chain, such as the user's inner call in a relayed transaction,
    /// leaves it `true`: see [`receipt_failures`](Self::receipt_failures).
    pub fn is_success(&self) -> bool {
        matches!(&self.status, FinalExecutionStatus::SuccessValue(_))
    }

    /// Whether the transaction's top-level [`status`](Self::status) is a
    /// failure: an action on the main chain of receipts failed, or the
    /// transaction was rejected when its chunk was applied.
    ///
    /// Like [`is_success`](Self::is_success), this doesn't see receipts that
    /// fail off the main chain; see [`receipt_failures`](Self::receipt_failures).
    pub fn is_failure(&self) -> bool {
        matches!(&self.status, FinalExecutionStatus::Failure(_))
    }

    /// Get the failure message if present.
    pub fn failure_message(&self) -> Option<String> {
        self.failure_error().map(ToString::to_string)
    }

    /// Get the typed execution error if present.
    pub fn failure_error(&self) -> Option<&TxExecutionError> {
        match &self.status {
            FinalExecutionStatus::Failure(err) => Some(err),
            _ => None,
        }
    }

    /// Every failed receipt's action error, in the order the RPC lists the
    /// receipt outcomes.
    ///
    /// [`is_failure`](Self::is_failure) and [`result`](Self::result) only
    /// see [`status`](Self::status), the result of the transaction's main
    /// chain of receipts. A receipt that fails off that chain doesn't change
    /// it. For a relayed transaction (an `Action::delegate`), `is_failure()`
    /// is `false` and `result()` is `Ok` even when the user's inner actions
    /// failed, while `receipt_failures()` is not empty. The same goes for a
    /// detached promise or a cross-contract call whose callback handled the
    /// failure.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use near_kit_primitives::rpc::FinalExecutionOutcome;
    /// fn check_relayed(outcome: &FinalExecutionOutcome) -> Result<(), String> {
    ///     if let Some(error) = outcome.receipt_failures().next() {
    ///         return Err(format!("an inner action failed: {error}"));
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn receipt_failures(&self) -> impl Iterator<Item = &ActionError> {
        self.receipts_outcome
            .iter()
            .filter_map(|receipt| receipt.outcome.status.failure())
    }

    /// Get the transaction hash.
    pub fn transaction_hash(&self) -> &CryptoHash {
        &self.transaction_outcome.id
    }

    /// Every log line the transaction emitted: the transaction outcome's
    /// logs, then each receipt outcome's logs in the order the RPC lists them.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use near_kit_primitives::rpc::FinalExecutionOutcome;
    /// fn events(outcome: &FinalExecutionOutcome) -> Vec<&str> {
    ///     outcome
    ///         .logs()
    ///         .filter_map(|log| log.strip_prefix("EVENT_JSON:"))
    ///         .collect()
    /// }
    /// ```
    pub fn logs(&self) -> impl Iterator<Item = &str> {
        std::iter::once(&self.transaction_outcome)
            .chain(&self.receipts_outcome)
            .flat_map(|outcome| outcome.outcome.logs.iter().map(String::as_str))
    }

    /// Get total gas used across all receipts.
    pub fn total_gas_used(&self) -> Gas {
        let tx_gas = self.transaction_outcome.outcome.gas_burnt.as_gas();
        let receipt_gas: u64 = self
            .receipts_outcome
            .iter()
            .map(|r| r.outcome.gas_burnt.as_gas())
            .sum();
        Gas::from_gas(tx_gas + receipt_gas)
    }

    /// The return value as raw bytes, or the failure as an `Err`.
    ///
    /// A send returns `Ok(outcome)` even when an action failed, so
    /// `.await?.result()?` is the usual way to make a failed action an error:
    /// [`OutcomeError::ActionFailed`] (or `InvalidTx` for a transaction
    /// rejected when its chunk was applied). It is also `Err` if the status
    /// is not a finished `SuccessValue`, or the value is not valid base64. A
    /// method that returns nothing gives an empty `Vec`.
    ///
    /// Like [`is_failure`](Self::is_failure), it reads the top-level
    /// [`status`](Self::status) only; check
    /// [`receipt_failures`](Self::receipt_failures) for receipts that fail
    /// off the main chain, such as a relayed inner call.
    pub fn result(&self) -> Result<Vec<u8>, OutcomeError> {
        match &self.status {
            FinalExecutionStatus::Failure(TxExecutionError::InvalidTxError(e)) => {
                Err(OutcomeError::InvalidTx(Box::new(e.clone())))
            }
            FinalExecutionStatus::Failure(TxExecutionError::ActionError(e)) => {
                Err(OutcomeError::ActionFailed(Box::new(e.clone())))
            }
            FinalExecutionStatus::SuccessValue(s) => STANDARD
                .decode(s)
                .map_err(|e| OutcomeError::InvalidBase64(e.to_string())),
            other => Err(OutcomeError::NotSuccess(Box::new(other.clone()))),
        }
    }

    /// Deserialize the return value as JSON.
    ///
    /// Returns `Err` if `result()` fails (on-chain failure, unexpected status,
    /// or invalid base64) or if JSON deserialization fails.
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, OutcomeError> {
        let bytes = self.result()?;
        // Empty SuccessValue (void change method) -> treat as null.
        let bytes: &[u8] = if bytes.is_empty() { b"null" } else { &bytes };
        Ok(serde_json::from_slice(bytes)?)
    }
}

/// Per-receipt execution status.
///
/// Matches nearcore's `ExecutionStatusView`. Used in [`ExecutionOutcome`].
///
/// A receipt fails with an action error ([`Failure`](Self::Failure)). A
/// transaction outcome can also fail with an [`InvalidTxError`]
/// ([`InvalidTx`](Self::InvalidTx)) when the transaction was included in a
/// chunk but rejected when the chunk was applied; such a transaction resolves
/// to `OutcomeError::InvalidTx` (`near_kit::Error::InvalidTx`).
///
/// The NEAR RPC serialises a failure as `{"Failure": {"ActionError": {…}}}` or
/// `{"Failure": {"InvalidTxError": {…}}}`. A custom [`Deserialize`] impl
/// unwraps the outer `TxExecutionError` envelope.
#[derive(Debug, Clone)]
pub enum ExecutionStatus {
    /// The execution is pending or unknown.
    Unknown,
    /// Execution failed with an action error.
    Failure(ActionError),
    /// The transaction was rejected when its chunk was applied.
    InvalidTx(InvalidTxError),
    /// Execution succeeded with a return value.
    SuccessValue(Vec<u8>),
    /// Execution succeeded, producing a receipt.
    SuccessReceiptId(CryptoHash),
}

impl ExecutionStatus {
    /// Whether the receipt succeeded, with a return value or a new receipt.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::SuccessValue(_) | Self::SuccessReceiptId(_))
    }

    /// Whether the execution failed, with an action error or an
    /// [`InvalidTx`](Self::InvalidTx) rejection.
    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failure(_) | Self::InvalidTx(_))
    }

    /// The action error, if the receipt failed.
    pub fn failure(&self) -> Option<&ActionError> {
        match self {
            Self::Failure(error) => Some(error),
            _ => None,
        }
    }
}

impl<'de> serde::Deserialize<'de> for ExecutionStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Mirror of the on-wire format that serde can derive.
        #[serde_as]
        #[derive(Deserialize)]
        enum Raw {
            Unknown,
            Failure(TxExecutionError),
            SuccessValue(#[serde_as(as = "Base64")] Vec<u8>),
            SuccessReceiptId(CryptoHash),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Unknown => Ok(Self::Unknown),
            Raw::Failure(TxExecutionError::ActionError(e)) => Ok(Self::Failure(e)),
            Raw::Failure(TxExecutionError::InvalidTxError(e)) => Ok(Self::InvalidTx(e)),
            Raw::SuccessValue(v) => Ok(Self::SuccessValue(v)),
            Raw::SuccessReceiptId(h) => Ok(Self::SuccessReceiptId(h)),
        }
    }
}

/// Transaction view in outcome.
#[derive(Debug, Clone, Deserialize)]
pub struct TransactionView {
    /// Signer account.
    pub signer_id: AccountId,
    /// Signer public key.
    pub public_key: PublicKey,
    /// Transaction nonce.
    pub nonce: u64,
    /// Receiver account.
    pub receiver_id: AccountId,
    /// Transaction hash.
    pub hash: CryptoHash,
    /// Actions in the transaction.
    #[serde(default)]
    pub actions: Vec<ActionView>,
    /// Transaction signature.
    pub signature: Signature,
    /// Priority fee (optional, for congestion pricing).
    pub priority_fee: Option<u64>,
    /// Nonce index (for gas key multi-nonce support).
    pub nonce_index: Option<u16>,
    /// Nonce validation mode (`None` from nodes older than nearcore 2.12; means monotonic).
    pub nonce_mode: Option<NonceMode>,
}

/// Backward-compatible deserialization helper for `GlobalContractIdentifierView`.
///
/// Handles the view format (`{"hash": "<base58>"}` / `{"account_id": "alice.near"}`),
/// the pre-2.12 identifier format (`{"CodeHash": "<base58>"}` / `{"AccountId":
/// "alice.near"}`, used by `NO_GLOBAL_CONTRACT_CODE` errors before nearcore#15539),
/// and the deprecated format (bare string `"<base58>"` / `"alice.near"`).
#[derive(Deserialize)]
#[serde(untagged)]
enum GlobalContractIdCompat {
    CodeHash {
        #[serde(alias = "CodeHash")]
        hash: CryptoHash,
    },
    AccountId {
        #[serde(alias = "AccountId")]
        account_id: AccountId,
    },
    DeprecatedCodeHash(CryptoHash),
    DeprecatedAccountId(AccountId),
}

/// Global contract identifier in RPC view responses.
///
/// Identifies a global contract either by its code hash (immutable) or by the
/// publishing account ID (updatable). Supports both the current and deprecated
/// JSON serialization formats from nearcore.
#[derive(Debug, Clone, Deserialize)]
#[serde(from = "GlobalContractIdCompat")]
pub enum GlobalContractIdentifierView {
    /// Referenced by code hash.
    CodeHash(CryptoHash),
    /// Referenced by publisher account ID.
    AccountId(AccountId),
}

impl From<GlobalContractIdCompat> for GlobalContractIdentifierView {
    fn from(compat: GlobalContractIdCompat) -> Self {
        match compat {
            GlobalContractIdCompat::CodeHash { hash }
            | GlobalContractIdCompat::DeprecatedCodeHash(hash) => Self::CodeHash(hash),
            GlobalContractIdCompat::AccountId { account_id }
            | GlobalContractIdCompat::DeprecatedAccountId(account_id) => {
                Self::AccountId(account_id)
            }
        }
    }
}

impl std::fmt::Display for GlobalContractIdentifierView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CodeHash(hash) => write!(f, "hash({hash})"),
            Self::AccountId(account_id) => write!(f, "{account_id}"),
        }
    }
}

/// View of a delegate action in RPC responses.
///
/// The node renders the inner actions as protocol [`Action`]s, not as
/// [`ActionView`]s: an inner deploy carries the full WASM, not its hash.
#[derive(Debug, Clone, Deserialize)]
pub struct DelegateActionView {
    /// The account that signed the delegate action.
    pub sender_id: AccountId,
    /// The intended receiver of the inner actions.
    pub receiver_id: AccountId,
    /// The actions to execute, in the node's protocol [`Action`] JSON.
    pub actions: Vec<Action>,
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Maximum block height before this delegate action expires.
    pub max_block_height: u64,
    /// Public key of the signer.
    pub public_key: PublicKey,
}

/// Nonce as it appears in a `DelegateV2` action view.
///
/// Externally tagged to match the node's JSON for nearcore's `TransactionNonce`:
/// `{"Nonce": {"nonce": N}}` for an ordinary key, or
/// `{"GasKeyNonce": {"nonce": N, "nonce_index": I}}` for a gas key.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub enum TransactionNonceView {
    /// Ordinary access-key nonce.
    Nonce {
        /// The nonce value.
        nonce: u64,
    },
    /// Gas-key nonce selecting one of the key's parallel nonces.
    GasKeyNonce {
        /// The nonce value for the selected parallel nonce.
        nonce: u64,
        /// Which parallel nonce this advances.
        nonce_index: u16,
    },
}

/// View of a `DelegateActionV2` (gas-key meta-transaction) in RPC responses.
///
/// Mirrors nearcore's `DelegateActionV2`: like [`DelegateActionView`] but its
/// `nonce` is a gas-key-capable [`TransactionNonceView`].
#[derive(Debug, Clone, Deserialize)]
pub struct DelegateActionV2View {
    /// The account that signed the delegate action.
    pub sender_id: AccountId,
    /// The intended receiver of the inner actions.
    pub receiver_id: AccountId,
    /// The actions to execute, in the node's protocol [`Action`] JSON.
    pub actions: Vec<Action>,
    /// Nonce for replay protection (gas-key capable).
    pub nonce: TransactionNonceView,
    /// Maximum block height before this delegate action expires.
    pub max_block_height: u64,
    /// Public key of the signer.
    pub public_key: PublicKey,
}

/// Versioned delegate-action payload as it appears in a `DelegateV2` view.
///
/// Externally tagged to match the node's JSON for nearcore's
/// `VersionedDelegateActionPayload`: `{"V2": { ...DelegateActionV2View... }}`.
#[derive(Debug, Clone, Deserialize)]
pub enum VersionedDelegateActionPayloadView {
    /// The V2 (gas-key) delegate action.
    V2(DelegateActionV2View),
}

/// Action view in transaction.
///
/// This is the node's JSON rendering of an [`Action`], as found in
/// `tx_status` responses ([`TransactionView::actions`]) and in action receipts.
/// Non-deploy variants can be turned back into an [`Action`] with
/// [`TryFrom<ActionView>`](Action#impl-TryFrom<ActionView>-for-Action). Deploy
/// variants are rejected because the view omits the contract code.
///
/// The inner actions of a [`Delegate`](Self::Delegate) or
/// [`DelegateV2`](Self::DelegateV2) view are not `ActionView`s: the node sends
/// them as protocol [`Action`]s (see [`DelegateActionView`]).
///
/// # Deploy views carry the code *hash*, not the WASM
///
/// [`DeployContract`](Self::DeployContract),
/// [`DeployGlobalContract`](Self::DeployGlobalContract) and
/// [`DeployGlobalContractByAccountId`](Self::DeployGlobalContractByAccountId)
/// do **not** contain the contract code. The node replaces the WASM with the
/// base64 of its 32-byte SHA-256 code hash before serving the view (nearcore
/// `views.rs`: `hash(&action.code).as_ref().to_vec()`). Use
/// [`deploy_code_hash`](Self::deploy_code_hash) to read it as a
/// [`CryptoHash`]. Converting the view into an [`Action`] returns
/// [`ActionViewConversionError::DeployCodeUnavailable`]; do not treat the
/// decoded bytes as WASM, and do not hash them again.
#[derive(Debug, Clone, Deserialize)]
pub enum ActionView {
    /// Create the receiver account.
    CreateAccount,
    /// Deploy a contract to the account.
    DeployContract {
        /// Base64 of the 32-byte code hash (not the WASM).
        code: String,
    },
    /// Call a contract method.
    FunctionCall {
        /// The method called.
        method_name: String,
        /// The call's arguments, base64-encoded.
        args: String,
        /// Gas attached to the call.
        gas: Gas,
        /// NEAR attached to the call.
        deposit: NearToken,
    },
    /// Transfer NEAR to the receiver.
    Transfer {
        /// The amount transferred.
        deposit: NearToken,
    },
    /// Stake NEAR as a validator.
    Stake {
        /// The total amount to stake.
        stake: NearToken,
        /// The validator key.
        public_key: PublicKey,
    },
    /// Add an access key to the account.
    AddKey {
        /// The key added.
        public_key: PublicKey,
        /// Its nonce and permission.
        access_key: AccessKeyDetails,
    },
    /// Delete an access key from the account.
    DeleteKey {
        /// The key deleted.
        public_key: PublicKey,
    },
    /// Delete the account and send its balance to `beneficiary_id`.
    DeleteAccount {
        /// The account that receives the remaining balance.
        beneficiary_id: AccountId,
    },
    /// A signed delegate action (NEP-366 meta-transaction) submitted by a
    /// relayer.
    Delegate {
        /// The delegate action.
        delegate_action: DelegateActionView,
        /// The delegate's signature over it.
        signature: Signature,
    },
    /// A versioned signed delegate action.
    DelegateV2 {
        /// The versioned delegate action payload.
        delegate_action: VersionedDelegateActionPayloadView,
        /// The delegate's signature over it.
        signature: Signature,
    },
    /// Publish a global contract, identified by its code hash.
    DeployGlobalContract {
        /// Base64 of the 32-byte code hash (not the WASM).
        code: String,
    },
    /// Publish a global contract, identified by the publisher's account ID.
    DeployGlobalContractByAccountId {
        /// Base64 of the 32-byte code hash (not the WASM).
        code: String,
    },
    /// Use a global contract published under a code hash as the account's
    /// contract.
    UseGlobalContract {
        /// The global contract's code hash.
        code_hash: CryptoHash,
    },
    /// Use the global contract published by an account as the account's
    /// contract.
    UseGlobalContractByAccountId {
        /// The publishing account.
        account_id: AccountId,
    },
    /// Initialize an account whose ID is derived from its contract and
    /// initial state (NEP-616).
    DeterministicStateInit {
        /// The global contract the account uses.
        code: GlobalContractIdentifierView,
        /// Initial storage entries, base64 key to base64 value.
        #[serde(default)]
        data: BTreeMap<String, String>,
        /// NEAR attached to the account.
        deposit: NearToken,
    },
    /// Transfer NEAR to a gas key.
    TransferToGasKey {
        /// The gas key funded.
        public_key: PublicKey,
        /// The amount transferred.
        deposit: NearToken,
    },
    /// Withdraw NEAR from a gas key.
    WithdrawFromGasKey {
        /// The gas key withdrawn from.
        public_key: PublicKey,
        /// The amount withdrawn.
        amount: NearToken,
    },
    /// Create a `0u` universal account from its state init.
    UniversalStateInit {
        /// Base64 of the borsh-encoded `UniversalStateInit` bytes.
        state_init: String,
        /// NEAR attached to the account.
        deposit: NearToken,
    },
}

impl From<AccessKeyPermissionView> for AccessKeyPermission {
    fn from(view: AccessKeyPermissionView) -> Self {
        match view {
            AccessKeyPermissionView::FullAccess => Self::FullAccess,
            AccessKeyPermissionView::FunctionCall {
                allowance,
                receiver_id,
                method_names,
            } => Self::FunctionCall(FunctionCallPermission {
                allowance,
                receiver_id,
                method_names,
            }),
            AccessKeyPermissionView::GasKeyFunctionCall {
                balance,
                num_nonces,
                allowance,
                receiver_id,
                method_names,
            } => Self::GasKeyFunctionCall(
                GasKeyInfo {
                    balance,
                    num_nonces,
                },
                FunctionCallPermission {
                    allowance,
                    receiver_id,
                    method_names,
                },
            ),
            AccessKeyPermissionView::GasKeyFullAccess {
                balance,
                num_nonces,
            } => Self::GasKeyFullAccess(GasKeyInfo {
                balance,
                num_nonces,
            }),
        }
    }
}

impl From<AccessKeyDetails> for AccessKey {
    fn from(view: AccessKeyDetails) -> Self {
        Self {
            nonce: view.nonce,
            permission: view.permission.into(),
        }
    }
}

/// Drops the block metadata (`block_height` / `block_hash`) and keeps the
/// nonce and permission.
impl From<AccessKeyView> for AccessKey {
    fn from(view: AccessKeyView) -> Self {
        Self {
            nonce: view.nonce,
            permission: view.permission.into(),
        }
    }
}

impl From<GlobalContractIdentifierView> for GlobalContractId {
    fn from(view: GlobalContractIdentifierView) -> Self {
        match view {
            GlobalContractIdentifierView::CodeHash(hash) => Self::CodeHash(*hash.as_bytes()),
            GlobalContractIdentifierView::AccountId(account_id) => Self::AccountId(account_id),
        }
    }
}

impl From<TransactionNonceView> for TransactionNonce {
    fn from(view: TransactionNonceView) -> Self {
        match view {
            TransactionNonceView::Nonce { nonce } => Self::from_nonce(nonce),
            TransactionNonceView::GasKeyNonce { nonce, nonce_index } => {
                Self::from_nonce_and_index(nonce, nonce_index)
            }
        }
    }
}

impl ActionView {
    /// The code hash carried by a `DeployContract` / `DeployGlobalContract` /
    /// `DeployGlobalContractByAccountId` view; `None` for every other variant.
    ///
    /// Decodes `code` (see the
    /// [enum-level note](ActionView#deploy-views-carry-the-code-hash-not-the-wasm)),
    /// failing if it is not valid base64 or not exactly 32 bytes.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::CryptoHash;
    /// use near_kit_primitives::rpc::ActionView;
    ///
    /// // `code` is base64(sha256(wasm)); here the WASM is the empty byte string.
    /// let view: ActionView = serde_json::from_value(serde_json::json!({
    ///     "DeployContract": { "code": "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=" }
    /// }))?;
    ///
    /// let hash = view.deploy_code_hash().expect("a deploy view")?;
    /// assert_eq!(hash, CryptoHash::hash(b""));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn deploy_code_hash(&self) -> Option<Result<CryptoHash, ActionViewConversionError>> {
        match self {
            Self::DeployContract { code }
            | Self::DeployGlobalContract { code }
            | Self::DeployGlobalContractByAccountId { code } => Some(decode_code_hash(code)),
            _ => None,
        }
    }
}

/// Decode a base64 view field, naming the field in the error.
fn decode_view_base64(field: &'static str, s: &str) -> Result<Vec<u8>, ActionViewConversionError> {
    STANDARD
        .decode(s)
        .map_err(|source| ActionViewConversionError::InvalidBase64 { field, source })
}

/// Decode the `code` field of a deploy view into the 32-byte code hash it holds.
fn decode_code_hash(code: &str) -> Result<CryptoHash, ActionViewConversionError> {
    let bytes = decode_view_base64("code", code)?;
    CryptoHash::try_from(bytes.as_slice()).map_err(|_| ActionViewConversionError::InvalidCodeHash {
        actual: bytes.len(),
    })
}

/// Wrap the inner actions of a delegate view, rejecting nested delegates.
fn non_delegate_actions(
    actions: Vec<Action>,
) -> Result<Vec<NonDelegateAction>, ActionViewConversionError> {
    actions
        .into_iter()
        .map(|action| {
            NonDelegateAction::from_action(action).ok_or(ActionViewConversionError::NestedDelegate)
        })
        .collect()
}

impl TryFrom<DelegateActionView> for DelegateAction {
    type Error = ActionViewConversionError;

    fn try_from(view: DelegateActionView) -> Result<Self, Self::Error> {
        Ok(Self {
            sender_id: view.sender_id,
            receiver_id: view.receiver_id,
            actions: non_delegate_actions(view.actions)?,
            nonce: view.nonce,
            max_block_height: view.max_block_height,
            public_key: view.public_key,
        })
    }
}

impl TryFrom<DelegateActionV2View> for DelegateActionV2 {
    type Error = ActionViewConversionError;

    fn try_from(view: DelegateActionV2View) -> Result<Self, Self::Error> {
        Ok(Self {
            sender_id: view.sender_id,
            receiver_id: view.receiver_id,
            actions: non_delegate_actions(view.actions)?,
            nonce: view.nonce.into(),
            max_block_height: view.max_block_height,
            public_key: view.public_key,
        })
    }
}

impl TryFrom<VersionedDelegateActionPayloadView> for VersionedDelegateActionPayload {
    type Error = ActionViewConversionError;

    fn try_from(view: VersionedDelegateActionPayloadView) -> Result<Self, Self::Error> {
        match view {
            VersionedDelegateActionPayloadView::V2(d) => Ok(Self::V2(d.try_into()?)),
        }
    }
}

/// Convert a node-reported action view back into a wire [`Action`], e.g. to
/// re-sign or replay a transaction fetched with `tx_status`.
///
/// # Errors
///
/// * A deploy view, which carries only the code hash (see [`ActionView`]) →
///   [`ActionViewConversionError::DeployCodeUnavailable`]. Fetch the code
///   separately, for example with `view_code` on an archival node.
/// * Invalid base64 in `args`, `code`, `data`, or `state_init` →
///   [`ActionViewConversionError::InvalidBase64`].
/// * A delegate action nested inside a `Delegate` / `DelegateV2` view →
///   [`ActionViewConversionError::NestedDelegate`].
///
/// # Example
///
/// The `transaction.actions` of a `tx_status` response, converted back to
/// [`Action`]s (one action shown, deserialized from the JSON the node returns):
///
/// ```rust
/// use near_kit_primitives::{Gas, NearToken};
/// use near_kit_primitives::protocol::Action;
/// use near_kit_primitives::rpc::ActionView;
///
/// let view: ActionView = serde_json::from_value(serde_json::json!({
///     "FunctionCall": {
///         "method_name": "set_greeting",
///         "args": "eyJncmVldGluZyI6ImhpIn0=",
///         "gas": 30000000000000u64,
///         "deposit": "0"
///     }
/// }))?;
///
/// let action = Action::try_from(view)?;
/// assert_eq!(
///     action,
///     Action::function_call(
///         "set_greeting",
///         br#"{"greeting":"hi"}"#.to_vec(),
///         Gas::from_tgas(30),
///         NearToken::from_yoctonear(0),
///     )
/// );
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// From a full outcome, such as the one near-kit's
/// `near.tx_status(&tx_hash, sender).wait_until::<Final>().await?` returns:
///
/// ```rust
/// use near_kit_primitives::protocol::{Action, ActionViewConversionError};
/// use near_kit_primitives::rpc::FinalExecutionOutcome;
///
/// fn actions_of(outcome: FinalExecutionOutcome) -> Result<Vec<Action>, ActionViewConversionError> {
///     outcome.transaction.actions.into_iter().map(Action::try_from).collect()
/// }
/// ```
impl TryFrom<ActionView> for Action {
    type Error = ActionViewConversionError;

    fn try_from(view: ActionView) -> Result<Self, Self::Error> {
        Ok(match view {
            ActionView::CreateAccount => Self::CreateAccount(CreateAccountAction),
            ActionView::DeployContract { code }
            | ActionView::DeployGlobalContract { code }
            | ActionView::DeployGlobalContractByAccountId { code } => {
                return Err(ActionViewConversionError::DeployCodeUnavailable {
                    code_hash: decode_code_hash(&code)?,
                });
            }
            ActionView::FunctionCall {
                method_name,
                args,
                gas,
                deposit,
            } => Self::FunctionCall(FunctionCallAction {
                method_name,
                args: decode_view_base64("args", &args)?,
                gas,
                deposit,
            }),
            ActionView::Transfer { deposit } => Self::Transfer(TransferAction { deposit }),
            ActionView::Stake { stake, public_key } => {
                Self::Stake(StakeAction { stake, public_key })
            }
            ActionView::AddKey {
                public_key,
                access_key,
            } => Self::AddKey(AddKeyAction {
                public_key,
                access_key: access_key.into(),
            }),
            ActionView::DeleteKey { public_key } => Self::DeleteKey(DeleteKeyAction { public_key }),
            ActionView::DeleteAccount { beneficiary_id } => {
                Self::DeleteAccount(DeleteAccountAction { beneficiary_id })
            }
            ActionView::Delegate {
                delegate_action,
                signature,
            } => Self::Delegate(Box::new(SignedDelegateAction {
                delegate_action: delegate_action.try_into()?,
                signature,
            })),
            ActionView::DelegateV2 {
                delegate_action,
                signature,
            } => Self::DelegateV2(Box::new(VersionedSignedDelegateAction {
                delegate_action: delegate_action.try_into()?,
                signature,
            })),
            ActionView::UseGlobalContract { code_hash } => {
                Self::UseGlobalContract(UseGlobalContractAction {
                    contract_identifier: GlobalContractId::CodeHash(*code_hash.as_bytes()),
                })
            }
            ActionView::UseGlobalContractByAccountId { account_id } => {
                Self::UseGlobalContract(UseGlobalContractAction {
                    contract_identifier: GlobalContractId::AccountId(account_id),
                })
            }
            ActionView::DeterministicStateInit {
                code,
                data,
                deposit,
            } => {
                let data = data
                    .into_iter()
                    .map(|(k, v)| {
                        Ok((
                            decode_view_base64("data", &k)?,
                            decode_view_base64("data", &v)?,
                        ))
                    })
                    .collect::<Result<BTreeMap<_, _>, ActionViewConversionError>>()?;
                Self::DeterministicStateInit(DeterministicStateInitAction {
                    state_init: StateInit::V1(StateInitV1 {
                        code: code.into(),
                        data,
                    }),
                    deposit,
                })
            }
            ActionView::TransferToGasKey {
                public_key,
                deposit,
            } => Self::TransferToGasKey(TransferToGasKeyAction {
                public_key,
                deposit,
            }),
            ActionView::WithdrawFromGasKey { public_key, amount } => {
                Self::WithdrawFromGasKey(WithdrawFromGasKeyAction { public_key, amount })
            }
            ActionView::UniversalStateInit {
                state_init,
                deposit,
            } => Self::UniversalStateInit(UniversalStateInitAction {
                state_init: decode_view_base64("state_init", &state_init)?,
                deposit,
            }),
        })
    }
}

/// Like [`TryFrom<ActionView> for Action`](Action#impl-TryFrom<ActionView>-for-Action),
/// additionally rejecting `Delegate` / `DelegateV2` views with
/// [`ActionViewConversionError::NestedDelegate`].
impl TryFrom<ActionView> for NonDelegateAction {
    type Error = ActionViewConversionError;

    fn try_from(view: ActionView) -> Result<Self, Self::Error> {
        Self::from_action(Action::try_from(view)?).ok_or(ActionViewConversionError::NestedDelegate)
    }
}

/// Merkle path item for cryptographic proofs.
#[derive(Debug, Clone, Deserialize)]
pub struct MerklePathItem {
    /// Hash at this node.
    pub hash: CryptoHash,
    /// Direction of the path.
    pub direction: MerkleDirection,
}

/// Direction in merkle path.
#[derive(Debug, Clone, Deserialize)]
pub enum MerkleDirection {
    /// The sibling hash is on the left.
    Left,
    /// The sibling hash is on the right.
    Right,
}

/// Execution outcome with ID.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionOutcomeWithId {
    /// Receipt or transaction ID.
    pub id: CryptoHash,
    /// Outcome details.
    pub outcome: ExecutionOutcome,
    /// Proof of execution.
    #[serde(default)]
    pub proof: Vec<MerklePathItem>,
    /// Block hash where this was executed.
    pub block_hash: CryptoHash,
}

/// Execution outcome details.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionOutcome {
    /// Executor account.
    pub executor_id: AccountId,
    /// Gas burnt during execution.
    pub gas_burnt: Gas,
    /// Tokens burnt for gas.
    pub tokens_burnt: NearToken,
    /// Logs emitted.
    pub logs: Vec<String>,
    /// Receipt IDs generated.
    pub receipt_ids: Vec<CryptoHash>,
    /// Execution status.
    pub status: ExecutionStatus,
    /// Execution metadata (gas profiling).
    pub metadata: Option<ExecutionMetadata>,
}

/// Execution metadata with gas profiling.
#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionMetadata {
    /// Metadata version (V1=1, V2=2, V3=3, V4=4).
    pub version: u32,
    /// Gas profile entries.
    pub gas_profile: Option<Vec<GasProfileEntry>>,
    /// The contract attached to the receiver account immediately before each
    /// action in the receipt ran — one entry per action, in order
    /// (`ExecutionMetadata` V4+, protocol 2.13).
    ///
    /// The inner `Option` is `None` (JSON `null`) when the account had no
    /// contract at that point (no code, or it did not yet exist). The outer
    /// `Option` is `None` when the field is absent (older V1–V3 metadata) or
    /// explicitly `null`.
    pub contracts: Option<Vec<Option<AccountContractView>>>,
}

/// The contract attached to an account, as reported per-action in
/// [`ExecutionMetadata::contracts`] (V4+).
///
/// A receipt action with no contract is represented as `None` in the
/// surrounding `Option`, so this enum only covers the cases where a contract
/// was present.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountContractView {
    /// A regular contract deployed to the account, identified by its code hash.
    Local(CryptoHash),
    /// A global contract referenced by its code hash.
    GlobalHash(CryptoHash),
    /// A global contract referenced by the account that deployed it.
    GlobalAccountId(AccountId),
}

/// Gas profile entry for detailed gas accounting.
#[derive(Debug, Clone, Deserialize)]
pub struct GasProfileEntry {
    /// Cost category (ACTION_COST or WASM_HOST_COST).
    pub cost_category: String,
    /// Cost name.
    pub cost: String,
    /// Gas used for this cost.
    pub gas_used: Gas,
}

/// A view function result and metadata from the same successful RPC response.
///
/// The default payload is raw bytes. Typed views using `with_metadata().await`
/// return this same structure with a decoded payload.
#[derive(Debug, Clone, Deserialize)]
pub struct ViewFunctionResult<T = Vec<u8>> {
    /// Raw return bytes or the decoded contract return value.
    pub result: T,
    /// Logs emitted during view call.
    pub logs: Vec<String>,
    /// Block height of the query.
    pub block_height: u64,
    /// Block hash of the query.
    pub block_hash: CryptoHash,
}

impl<T> ViewFunctionResult<T> {
    /// A result read at `block_height` / `block_hash`, with no logs.
    ///
    /// [`Viewer`](crate::Viewer) implementations build their results with
    /// this (and [`with_logs`](Self::with_logs)) rather than a struct
    /// literal, so fields can be added later without breaking them.
    pub fn new(result: T, block_height: u64, block_hash: CryptoHash) -> Self {
        Self {
            result,
            logs: Vec::new(),
            block_height,
            block_hash,
        }
    }

    /// Attach the logs the view emitted.
    pub fn with_logs(mut self, logs: Vec<String>) -> Self {
        self.logs = logs;
        self
    }
}

impl ViewFunctionResult {
    /// The result as UTF-8 text.
    pub fn text(&self) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(self.result.clone())
    }

    /// Deserialize the result as JSON.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::CryptoHash;
    /// use near_kit_primitives::rpc::ViewFunctionResult;
    ///
    /// #[derive(serde::Deserialize)]
    /// struct Stats {
    ///     count: u64,
    /// }
    ///
    /// // A view's raw result, as a `Viewer` returns it.
    /// let result = ViewFunctionResult::new(br#"{"count":3}"#.to_vec(), 1, CryptoHash::ZERO);
    /// let stats: Stats = result.json()?;
    /// assert_eq!(stats.count, 3);
    ///
    /// // A method that returns nothing decodes as `()`.
    /// let empty = ViewFunctionResult::new(Vec::new(), 1, CryptoHash::ZERO);
    /// let () = empty.json()?;
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        // NEAR returns zero bytes for `()`-returning (void) methods; map empty -> null so
        // `()` and `Option<T>` deserialize instead of failing with "EOF while parsing a value".
        let bytes: &[u8] = if self.result.is_empty() {
            b"null"
        } else {
            &self.result
        };
        serde_json::from_slice(bytes)
    }

    /// Deserialize the result as Borsh.
    ///
    /// Use this for contracts that return Borsh-encoded data instead of JSON.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::CryptoHash;
    /// use near_kit_primitives::rpc::ViewFunctionResult;
    ///
    /// let bytes = borsh::to_vec(&(7u32, true))?;
    /// let result = ViewFunctionResult::new(bytes, 1, CryptoHash::ZERO);
    /// let state: (u32, bool) = result.borsh()?;
    /// assert_eq!(state, (7, true));
    /// # Ok::<(), borsh::io::Error>(())
    /// ```
    pub fn borsh<T: borsh::BorshDeserialize>(&self) -> Result<T, borsh::io::Error> {
        borsh::from_slice(&self.result)
    }
}

/// Receipt from EXPERIMENTAL_tx_status.
#[derive(Debug, Clone, Deserialize)]
pub struct Receipt {
    /// Predecessor account that created this receipt.
    pub predecessor_id: AccountId,
    /// Receiver account for this receipt.
    pub receiver_id: AccountId,
    /// Receipt ID.
    pub receipt_id: CryptoHash,
    /// Receipt content (action or data).
    pub receipt: ReceiptContent,
    /// Priority (optional, for congestion pricing).
    pub priority: Option<u64>,
}

/// Receipt content - action, data, or global contract distribution.
#[derive(Debug, Clone, Deserialize)]
pub enum ReceiptContent {
    /// Action receipt.
    Action(ActionReceiptData),
    /// Data receipt.
    Data(DataReceiptData),
    /// Global contract distribution receipt.
    GlobalContractDistribution {
        /// Global contract identifier.
        id: GlobalContractIdentifierView,
        /// Target shard ID.
        target_shard: u64,
        /// Shards that have already received this contract.
        #[serde(default)]
        already_delivered_shards: Vec<u64>,
        /// Code bytes (base64).
        code: String,
        /// Nonce (present in v2 receipts).
        nonce: Option<u64>,
    },
}

/// Data receiver for output data in action receipts.
#[derive(Debug, Clone, Deserialize)]
pub struct DataReceiverView {
    /// Data ID.
    pub data_id: CryptoHash,
    /// Receiver account ID.
    pub receiver_id: AccountId,
}

/// Action receipt data.
#[derive(Debug, Clone, Deserialize)]
pub struct ActionReceiptData {
    /// Signer account ID.
    pub signer_id: AccountId,
    /// Signer public key.
    pub signer_public_key: PublicKey,
    /// Gas price for this receipt.
    pub gas_price: NearToken,
    /// Output data receivers.
    #[serde(default)]
    pub output_data_receivers: Vec<DataReceiverView>,
    /// Input data IDs.
    #[serde(default)]
    pub input_data_ids: Vec<CryptoHash>,
    /// Actions in this receipt.
    pub actions: Vec<ActionView>,
    /// Whether this is a promise yield.
    pub is_promise_yield: Option<bool>,
}

/// Data receipt data.
#[derive(Debug, Clone, Deserialize)]
pub struct DataReceiptData {
    /// Data ID.
    pub data_id: CryptoHash,
    /// Data content (optional).
    pub data: Option<String>,
}

/// Node status response.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    /// Protocol version.
    pub protocol_version: u32,
    /// Latest protocol version supported.
    pub latest_protocol_version: u32,
    /// Chain ID.
    pub chain_id: String,
    /// Genesis hash.
    pub genesis_hash: CryptoHash,
    /// RPC address.
    pub rpc_addr: Option<String>,
    /// Node public key.
    pub node_public_key: Option<String>,
    /// Node key (deprecated).
    pub node_key: Option<String>,
    /// Validator account ID (if validating).
    pub validator_account_id: Option<AccountId>,
    /// Validator public key (if validating).
    pub validator_public_key: Option<PublicKey>,
    /// List of current validators.
    #[serde(default)]
    pub validators: Vec<ValidatorInfo>,
    /// Sync information.
    pub sync_info: SyncInfo,
    /// Node version.
    pub version: NodeVersion,
    /// Uptime in seconds.
    pub uptime_sec: Option<u64>,
}

/// Validator information.
#[derive(Debug, Clone, Deserialize)]
pub struct ValidatorInfo {
    /// Validator account ID.
    pub account_id: AccountId,
}

/// Sync information.
#[derive(Debug, Clone, Deserialize)]
pub struct SyncInfo {
    /// Latest block hash.
    pub latest_block_hash: CryptoHash,
    /// Latest block height.
    pub latest_block_height: u64,
    /// Latest state root.
    pub latest_state_root: Option<CryptoHash>,
    /// Latest block timestamp.
    pub latest_block_time: String,
    /// Whether the node is syncing.
    pub syncing: bool,
    /// Earliest block hash (if available).
    pub earliest_block_hash: Option<CryptoHash>,
    /// Earliest block height (if available).
    pub earliest_block_height: Option<u64>,
    /// Earliest block time (if available).
    pub earliest_block_time: Option<String>,
    /// Current epoch ID.
    pub epoch_id: Option<CryptoHash>,
    /// Epoch start height.
    pub epoch_start_height: Option<u64>,
}

/// Node version information.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeVersion {
    /// Version string.
    pub version: String,
    /// Build string.
    pub build: String,
    /// Git commit hash.
    pub commit: Option<String>,
    /// Rust compiler version.
    pub rustc_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ActionErrorKind;
    use crate::signer::KeyType;

    const PK: &str = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp";
    const SIG: &str = "ed25519:3s1dvMqNDCByoMnDnkhB4GPjTSXCRt4nt3Af5n1RX8W7aJ2FC6MfRf5BNXZ52EBifNJnNVBsGvke6GRYuaEYJXt5";
    const TX_HASH: &str = "9FtHUFBQsZ2MG77K3x3MJ9wjX3UT8zE1TczCrhZEcG8U";
    const RECEIPT_ID: &str = "3GTGoiN3FEoJenSw5ob4YMmFEV2Fbiichj3FDBnM78xK";

    #[test]
    fn test_access_key_list_parses_ml_dsa65_hash_handle() {
        // A 2.13 node returns ML-DSA-65 access keys as a 32-byte `ml-dsa-65-hash:`
        // handle (the full pubkey is not stored on-trie). View parsing must
        // accept it without panicking — in the key list and as the `last_key`
        // pagination cursor, which is the same handle type.
        let handle = format!("ml-dsa-65-hash:{}", bs58::encode([3u8; 32]).into_string());
        let json = serde_json::json!({
            "keys": [
                {
                    "public_key": handle,
                    "access_key": { "nonce": 1u64, "permission": "FullAccess" }
                },
                {
                    "public_key": PK,
                    "access_key": { "nonce": 2u64, "permission": "FullAccess" }
                }
            ],
            "last_key": handle,
            "block_height": 42u64,
            "block_hash": "11111111111111111111111111111111"
        });

        let list: AccessKeyListView = serde_json::from_value(json).unwrap();
        assert_eq!(list.keys.len(), 2);
        assert!(list.keys[0].public_key.is_ml_dsa65_hash());
        assert!(list.keys[0].public_key.as_full().is_none());
        assert_eq!(list.keys[0].public_key.key_type(), KeyType::MlDsa65);
        assert_eq!(list.keys[0].public_key.to_string(), handle);
        // The ed25519 entry is a full, usable key.
        let ed = list.keys[1].public_key.as_full().unwrap();
        assert_eq!(ed.key_type(), KeyType::Ed25519);
        assert_eq!(ed.to_string(), PK);
        // The cursor round-trips to the exact wire string, so it can be sent
        // back verbatim as `after_key`.
        let last_key = list.last_key.expect("last_key present");
        assert_eq!(last_key, list.keys[0].public_key);
        assert_eq!(last_key.to_string(), handle);
    }

    #[test]
    fn test_access_key_list_without_last_key_is_final_page() {
        // Pre-pagination nodes (and the last page of a paginated listing) omit
        // `last_key` entirely.
        let json = serde_json::json!({
            "keys": [],
            "block_height": 42u64,
            "block_hash": "11111111111111111111111111111111"
        });
        let list: AccessKeyListView = serde_json::from_value(json).unwrap();
        assert!(list.keys.is_empty());
        assert_eq!(list.last_key, None);
    }

    #[test]
    fn test_view_state_result_deserializes_with_cursor() {
        // `key`/`value`/`last_key` are base64 on the wire.
        let json = serde_json::json!({
            "values": [
                { "key": STANDARD.encode(b"STATE"), "value": STANDARD.encode(b"v1") },
                { "key": STANDARD.encode(b"k2"), "value": STANDARD.encode([0u8, 1, 2]) }
            ],
            "last_key": STANDARD.encode(b"k2"),
            "block_height": 12345,
            "block_hash": "11111111111111111111111111111111"
        });
        let result: ViewStateResult = serde_json::from_value(json).unwrap();
        assert_eq!(result.values.len(), 2);
        assert_eq!(result.values[0].key, b"STATE");
        assert_eq!(result.values[0].value, b"v1");
        assert_eq!(result.values[1].value, vec![0u8, 1, 2]);
        assert_eq!(result.last_key.as_deref(), Some(b"k2".as_slice()));
        assert_eq!(result.block_height, 12345);
        assert_eq!(
            result.block_hash,
            "11111111111111111111111111111111".parse().unwrap()
        );
    }

    #[test]
    fn test_view_state_result_last_page_has_no_cursor() {
        // The final page omits `last_key`.
        let json = serde_json::json!({
            "values": [{ "key": STANDARD.encode(b"k"), "value": STANDARD.encode(b"v") }],
            "block_height": 12345,
            "block_hash": "11111111111111111111111111111111"
        });
        let result: ViewStateResult = serde_json::from_value(json).unwrap();
        assert_eq!(result.values.len(), 1);
        assert!(result.last_key.is_none());
    }

    #[test]
    fn test_execution_metadata_v4_contracts_parsed() {
        // Captured from a 2.13 node: V4 metadata with one contract per action.
        let json = serde_json::json!({
            "version": 4,
            "gas_profile": [],
            "contracts": [
                null,
                { "local": "11111111111111111111111111111111" },
                { "global_hash": "11111111111111111111111111111111" },
                { "global_account_id": "contract.near" }
            ]
        });
        let meta: ExecutionMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.version, 4);
        let contracts = meta.contracts.expect("V4 carries contracts");
        assert_eq!(contracts.len(), 4);
        assert!(contracts[0].is_none());
        assert!(matches!(contracts[1], Some(AccountContractView::Local(_))));
        assert!(matches!(
            contracts[2],
            Some(AccountContractView::GlobalHash(_))
        ));
        match &contracts[3] {
            Some(AccountContractView::GlobalAccountId(id)) => {
                assert_eq!(id.as_str(), "contract.near")
            }
            other => panic!("expected global_account_id, got {other:?}"),
        }
    }

    #[test]
    fn test_execution_metadata_v3_has_no_contracts() {
        // V1-V3 omit `contracts`; parsing must stay backward-compatible.
        let json = serde_json::json!({
            "version": 3,
            "gas_profile": [
                { "cost_category": "WASM_HOST_COST", "cost": "BASE", "gas_used": "100" }
            ]
        });
        let meta: ExecutionMetadata = serde_json::from_value(json).unwrap();
        assert_eq!(meta.version, 3);
        assert!(meta.contracts.is_none());
        assert_eq!(meta.gas_profile.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_account_view_initialized_omits_state() {
        // Pre-2.14 nodes and initialized accounts omit both fields.
        let view: AccountView = serde_json::from_value(serde_json::json!({
            "amount": "1000000000000000000000000",
            "locked": "0",
            "code_hash": "11111111111111111111111111111111",
            "storage_usage": 182,
            "block_height": 100,
            "block_hash": "11111111111111111111111111111111"
        }))
        .unwrap();
        assert_eq!(view.state, AccountState::Initialized);
        assert_eq!(view.bootstrap_nonce, None);
    }

    #[test]
    fn test_account_view_uninitialized_universal_account() {
        let view: AccountView = serde_json::from_value(serde_json::json!({
            "amount": "1000000000000000000000000",
            "locked": "0",
            "code_hash": "11111111111111111111111111111111",
            "storage_usage": 100,
            "state": "uninitialized",
            "bootstrap_nonce": 100000000,
            "block_height": 100,
            "block_hash": "11111111111111111111111111111111"
        }))
        .unwrap();
        assert_eq!(view.state, AccountState::Uninitialized);
        assert_eq!(view.bootstrap_nonce, Some(100_000_000));
    }

    fn make_account_view(amount: u128, locked: u128, storage_usage: u64) -> AccountView {
        AccountView {
            amount: NearToken::from_yoctonear(amount),
            locked: NearToken::from_yoctonear(locked),
            code_hash: CryptoHash::default(),
            storage_usage,
            storage_paid_at: 0,
            global_contract_hash: None,
            global_contract_account_id: None,
            state: AccountState::Initialized,
            bootstrap_nonce: None,
            block_height: 0,
            block_hash: CryptoHash::default(),
        }
    }

    #[test]
    fn test_available_balance_no_stake_no_storage() {
        // No storage, no stake -> all balance is available
        let view = make_account_view(1_000_000_000_000_000_000_000_000, 0, 0); // 1 NEAR
        assert_eq!(view.available(), view.amount);
    }

    #[test]
    fn test_available_balance_with_storage_no_stake() {
        // 1000 bytes storage (= 0.00001 NEAR * 1000 = 0.01 NEAR = 10^22 yocto)
        // Amount: 1 NEAR = 10^24 yocto
        // Available should be: 1 NEAR - 0.01 NEAR = 0.99 NEAR
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let storage_usage = 1000u64;
        let storage_cost = STORAGE_AMOUNT_PER_BYTE * storage_usage as u128; // 10^22

        let view = make_account_view(amount, 0, storage_usage);
        let expected = NearToken::from_yoctonear(amount - storage_cost);
        assert_eq!(view.available(), expected);
    }

    #[test]
    fn test_available_balance_stake_covers_storage() {
        // Staked amount >= storage cost -> all liquid balance is available
        // 1000 bytes storage = 10^22 yocto cost
        // 1 NEAR staked = 10^24 yocto (more than storage cost)
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR liquid
        let locked = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR staked
        let storage_usage = 1000u64;

        let view = make_account_view(amount, locked, storage_usage);
        // All liquid balance should be available since stake covers storage
        assert_eq!(view.available(), view.amount);
    }

    #[test]
    fn test_available_balance_stake_partially_covers_storage() {
        // Staked = 0.005 NEAR = 5 * 10^21 yocto
        // Storage = 1000 bytes = 0.01 NEAR = 10^22 yocto
        // Reserved = 0.01 - 0.005 = 0.005 NEAR = 5 * 10^21 yocto
        // Amount = 1 NEAR
        // Available = 1 NEAR - 0.005 NEAR = 0.995 NEAR
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let locked = 5_000_000_000_000_000_000_000u128; // 0.005 NEAR
        let storage_usage = 1000u64;
        let storage_cost = STORAGE_AMOUNT_PER_BYTE * storage_usage as u128; // 10^22
        let reserved = storage_cost - locked; // 5 * 10^21

        let view = make_account_view(amount, locked, storage_usage);
        let expected = NearToken::from_yoctonear(amount - reserved);
        assert_eq!(view.available(), expected);
    }

    #[test]
    fn test_storage_cost_calculation() {
        let storage_usage = 1000u64;
        let view = make_account_view(1_000_000_000_000_000_000_000_000, 0, storage_usage);

        let expected_cost = STORAGE_AMOUNT_PER_BYTE * storage_usage as u128;
        assert_eq!(
            view.storage_cost(),
            NearToken::from_yoctonear(expected_cost)
        );
    }

    #[test]
    fn test_storage_cost_zero_when_stake_covers() {
        // Staked > storage cost -> storage_cost returns 0
        let locked = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let view = make_account_view(1_000_000_000_000_000_000_000_000, locked, 1000);

        assert_eq!(view.storage_cost(), NearToken::ZERO);
    }

    #[test]
    fn test_account_balance_from_view() {
        let amount = 1_000_000_000_000_000_000_000_000u128; // 1 NEAR
        let locked = 500_000_000_000_000_000_000_000u128; // 0.5 NEAR
        let storage_usage = 1000u64;

        let view = make_account_view(amount, locked, storage_usage);
        let balance = AccountBalance::from(view.clone());

        assert_eq!(balance.total, view.amount);
        assert_eq!(balance.available, view.available());
        assert_eq!(balance.locked, view.locked);
        assert_eq!(balance.storage_cost, view.storage_cost());
        assert_eq!(balance.storage_usage, storage_usage);
    }

    fn make_view_result(result: Vec<u8>) -> ViewFunctionResult {
        ViewFunctionResult::new(result, 12345, CryptoHash::default())
    }

    #[test]
    fn test_view_function_result_text() {
        let result = make_view_result(b"hello world".to_vec());
        assert_eq!(result.text().unwrap(), "hello world");
    }

    #[test]
    fn test_view_function_result_json_empty_option_is_none() {
        // Empty bytes map to JSON null, so `Option<T>` decodes to `None`.
        let result = make_view_result(vec![]);
        let value: Option<u64> = result.json().unwrap();
        assert_eq!(value, None);
    }

    #[test]
    fn test_view_function_result_json_non_empty_option_is_some() {
        // Non-empty payloads are unchanged: `42` decodes to both u64 and Some(42).
        let result = make_view_result(b"42".to_vec());
        let scalar: u64 = result.json().unwrap();
        assert_eq!(scalar, 42);
        let opt: Option<u64> = result.json().unwrap();
        assert_eq!(opt, Some(42));
    }

    #[test]
    fn test_gas_profile_entry_string_gas_used() {
        let json = serde_json::json!({
            "cost_category": "WASM_HOST_COST",
            "cost": "BASE",
            "gas_used": "123456789"
        });
        let entry: GasProfileEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.gas_used.as_gas(), 123456789);
    }

    #[test]
    fn test_gas_profile_entry_numeric_gas_used() {
        let json = serde_json::json!({
            "cost_category": "ACTION_COST",
            "cost": "FUNCTION_CALL",
            "gas_used": 999000000
        });
        let entry: GasProfileEntry = serde_json::from_value(json).unwrap();
        assert_eq!(entry.gas_used.as_gas(), 999000000);
    }

    #[test]
    fn congestion_info_gas_decodes_from_a_decimal_string() {
        let info: CongestionInfoView = serde_json::from_str(
            r#"{"delayed_receipts_gas":"340282366920938463463374607431768211455","buffered_receipts_gas":"7","receipt_bytes":3,"allowed_shard":1}"#,
        )
        .unwrap();
        assert_eq!(info.delayed_receipts_gas, u128::MAX);
        assert_eq!(info.buffered_receipts_gas, 7);

        let info: CongestionInfoView = serde_json::from_str("{}").unwrap();
        assert_eq!(info.delayed_receipts_gas, 0);
        assert_eq!(info.buffered_receipts_gas, 0);

        // nearcore always sends these as strings; a bare number isn't accepted.
        for bad in [
            r#"{"delayed_receipts_gas":"x"}"#,
            r#"{"delayed_receipts_gas":7}"#,
        ] {
            assert!(
                serde_json::from_str::<CongestionInfoView>(bad).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn test_delegate_v2_action_view_plain_nonce() {
        // The V2 view must also accept an ordinary (non-gas-key) nonce.
        let json = serde_json::json!({
            "DelegateV2": {
                "delegate_action": {
                    "V2": {
                        "sender_id": "alice.near",
                        "receiver_id": "contract.near",
                        "actions": [],
                        "nonce": { "Nonce": { "nonce": 7 } },
                        "max_block_height": 1,
                        "public_key": PK
                    }
                },
                "signature": SIG
            }
        });
        let action: ActionView = serde_json::from_value(json).unwrap();
        let ActionView::DelegateV2 {
            delegate_action: VersionedDelegateActionPayloadView::V2(d),
            ..
        } = action
        else {
            panic!("Expected DelegateV2 action");
        };
        assert_eq!(d.nonce, TransactionNonceView::Nonce { nonce: 7 });
    }

    #[test]
    fn test_final_execution_status_decodes() {
        let decode = |json| serde_json::from_value::<FinalExecutionStatus>(json).unwrap();
        assert!(matches!(
            decode(serde_json::json!("NotStarted")),
            FinalExecutionStatus::NotStarted
        ));
        assert!(matches!(
            decode(serde_json::json!("Started")),
            FinalExecutionStatus::Started
        ));
        assert!(matches!(
            decode(serde_json::json!({"SuccessValue": "aGVsbG8="})),
            FinalExecutionStatus::SuccessValue(s) if s == "aGVsbG8="
        ));
        assert!(matches!(
            decode(panicked("Smart contract panicked")),
            FinalExecutionStatus::Failure(_)
        ));
    }

    /// The JSON of an outcome whose top-level status is `status`, with one
    /// receipt outcome per entry of `receipts` (each a receipt status).
    /// `logs[0]` holds the transaction outcome's logs and `logs[i + 1]` those
    /// of receipt `i`; a missing entry logs nothing.
    fn outcome_json(
        status: serde_json::Value,
        receipts: &[serde_json::Value],
        logs: &[&[&str]],
    ) -> serde_json::Value {
        let outcome = |id: &str, status: &serde_json::Value, i: usize| {
            serde_json::json!({
                "id": id,
                "outcome": {
                    "executor_id": "alice.near",
                    "gas_burnt": 1,
                    "tokens_burnt": "0",
                    "logs": logs.get(i).copied().unwrap_or_default(),
                    "receipt_ids": [],
                    "status": status
                },
                "block_hash": "A6DJpKBhmAMmBuQXtY3dWbo8dGVSQ9yH7BQSJBfn8rBo",
                "proof": []
            })
        };
        let tx_status = serde_json::json!({"SuccessReceiptId": RECEIPT_ID});
        serde_json::json!({
            "status": status,
            "transaction": {
                "signer_id": "alice.near",
                "public_key": PK,
                "nonce": 1,
                "receiver_id": "bob.near",
                "actions": [],
                "signature": SIG,
                "hash": TX_HASH
            },
            "transaction_outcome": outcome(TX_HASH, &tx_status, 0),
            "receipts_outcome": receipts
                .iter()
                .enumerate()
                .map(|(i, status)| outcome(RECEIPT_ID, status, i + 1))
                .collect::<Vec<_>>(),
        })
    }

    fn outcome_with_receipts(
        status: serde_json::Value,
        receipts: &[serde_json::Value],
    ) -> FinalExecutionOutcome {
        serde_json::from_value(outcome_json(status, receipts, &[])).unwrap()
    }

    fn panicked(message: &str) -> serde_json::Value {
        serde_json::json!({"Failure": {"ActionError": {
            "index": 0,
            "kind": {"FunctionCallError": {"ExecutionError": message}}
        }}})
    }

    #[test]
    fn test_final_execution_outcome_logs_in_order() {
        let ok = serde_json::json!({"SuccessValue": ""});
        let json = outcome_json(
            ok.clone(),
            &[ok.clone(), ok.clone(), ok],
            &[&["tx"], &["a", "b"], &[], &["c"]],
        );
        let outcome: FinalExecutionOutcome = serde_json::from_value(json).unwrap();
        assert_eq!(outcome.logs().collect::<Vec<_>>(), ["tx", "a", "b", "c"]);
    }

    #[test]
    fn test_send_tx_response_pending_none() {
        let json = serde_json::json!({
            "final_execution_status": "NONE"
        });
        let response: RpcTransactionResponse = serde_json::from_value(json).unwrap();
        assert_eq!(response.final_execution_status, TxExecutionStatus::None);
        assert!(response.outcome.is_none());
        // `transaction_hash` isn't in the JSON; the rpc methods fill it in.
        assert!(response.transaction_hash.is_zero());
    }

    /// A relayed transaction whose inner call panicked: the delegate receipt
    /// succeeded, so the top-level status is a success, but the inner
    /// receipt's failure shows in `receipt_failures`.
    #[test]
    fn test_receipt_failures_relayed_inner_failure() {
        let ok = serde_json::json!({"SuccessValue": ""});
        let outcome = outcome_with_receipts(
            ok.clone(),
            &[
                serde_json::json!({"SuccessReceiptId": "A6DJpKBhmAMmBuQXtY3dWbo8dGVSQ9yH7BQSJBfn8rBo"}),
                panicked("inner call panicked"),
                ok,
            ],
        );
        assert!(!outcome.is_failure());
        assert!(outcome.is_success());
        assert!(outcome.result().is_ok());

        let failures: Vec<_> = outcome.receipt_failures().collect();
        assert_eq!(failures.len(), 1);
        assert!(
            matches!(
                &failures[0].kind,
                ActionErrorKind::FunctionCallError(e) if format!("{e:?}").contains("inner call panicked")
            ),
            "{failures:?}"
        );
    }

    /// A cross-contract call whose receipts failed: every failure is listed,
    /// in the order the RPC gives the receipt outcomes.
    #[test]
    fn test_receipt_failures_cross_contract_in_rpc_order() {
        let missing = serde_json::json!({"Failure": {"ActionError": {
            "index": 1,
            "kind": {"AccountDoesNotExist": {"account_id": "nobody.near"}}
        }}});
        let outcome = outcome_with_receipts(
            panicked("first"),
            &[
                serde_json::json!({"SuccessValue": ""}),
                panicked("first"),
                missing,
            ],
        );
        assert!(outcome.is_failure());

        let failures: Vec<_> = outcome.receipt_failures().collect();
        assert_eq!(failures.len(), 2);
        assert!(matches!(
            failures[0].kind,
            ActionErrorKind::FunctionCallError(_)
        ));
        assert_eq!(failures[1].index, Some(1));
        assert!(matches!(
            &failures[1].kind,
            ActionErrorKind::AccountDoesNotExist { account_id } if account_id.as_str() == "nobody.near"
        ));
    }

    #[test]
    fn test_receipt_failures_none() {
        let ok = serde_json::json!({"SuccessValue": ""});
        let outcome = outcome_with_receipts(ok.clone(), &[ok.clone(), ok]);
        assert!(outcome.is_success());
        assert_eq!(outcome.receipt_failures().count(), 0);

        // An outcome from `send_tx` with no receipt outcomes has none either.
        assert_eq!(make_success_outcome("").receipt_failures().count(), 0);
    }

    fn make_success_outcome(base64_value: &str) -> FinalExecutionOutcome {
        let mut json = outcome_json(serde_json::json!({"SuccessValue": base64_value}), &[], &[]);
        json["final_execution_status"] = "FINAL".into();
        let response: RpcTransactionResponse = serde_json::from_value(json).unwrap();
        assert_eq!(response.final_execution_status, TxExecutionStatus::Final);
        response.outcome.unwrap()
    }

    #[test]
    fn test_outcome_result() {
        // "hello" in base64
        let outcome = make_success_outcome("aGVsbG8=");
        assert_eq!(outcome.result().unwrap(), b"hello");
    }

    #[test]
    fn test_outcome_result_empty() {
        let outcome = make_success_outcome("");
        assert_eq!(outcome.result().unwrap(), b"");
    }

    #[test]
    fn test_outcome_json() {
        // JSON `42` in base64
        let outcome = make_success_outcome("NDI=");
        let val: u64 = outcome.json().unwrap();
        assert_eq!(val, 42);
    }

    #[test]
    fn test_outcome_json_empty_option_is_none() {
        // Empty SuccessValue maps to JSON null, so `Option<T>` decodes to `None`.
        let outcome = make_success_outcome("");
        let val: Option<u64> = outcome.json().unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_outcome_json_bad_data() {
        // "hello" is not valid JSON
        let outcome = make_success_outcome("aGVsbG8=");
        let result: Result<u64, _> = outcome.json();
        assert!(result.is_err());
    }

    #[test]
    fn test_outcome_result_invalid_base64_returns_err() {
        // If the RPC somehow returns invalid base64 (protocol bug),
        // result() returns an error so callers can distinguish it from empty values.
        let outcome = make_success_outcome("not-valid-base64!!!");
        let err = outcome.result().unwrap_err();
        assert!(
            err.to_string().contains("base64"),
            "Error should mention base64 decode failure, got: {err}"
        );
    }

    #[test]
    fn test_outcome_failure_result_returns_err() {
        let mut json = outcome_json(panicked("test error"), &[], &[]);
        json["final_execution_status"] = "EXECUTED_OPTIMISTIC".into();
        let response: RpcTransactionResponse = serde_json::from_value(json).unwrap();
        let outcome = response.outcome.unwrap();

        assert!(outcome.is_failure());
        assert!(!outcome.is_success());
        assert!(outcome.failure_message().is_some());
        assert!(outcome.failure_error().is_some());
        assert!(outcome.result().is_err());
        assert!(outcome.json::<u64>().is_err());
        // Metadata is still accessible
        assert!(!outcome.transaction_hash().is_zero());
        assert!(outcome.total_gas_used().as_gas() > 0);

        let success = make_success_outcome("");
        assert!(success.is_success() && !success.is_failure());
    }

    #[test]
    fn test_execution_status_unknown() {
        let json = serde_json::json!("Unknown");
        let status: ExecutionStatus = serde_json::from_value(json).unwrap();
        assert!(matches!(status, ExecutionStatus::Unknown));
    }

    #[test]
    fn test_execution_status_accessors() {
        let value: ExecutionStatus =
            serde_json::from_value(serde_json::json!({"SuccessValue": "aGVsbG8="})).unwrap();
        assert!(value.is_success() && !value.is_failure());
        assert!(matches!(&value, ExecutionStatus::SuccessValue(v) if v == b"hello"));
        assert!(value.failure().is_none());

        let receipt: ExecutionStatus =
            serde_json::from_value(serde_json::json!({"SuccessReceiptId": TX_HASH})).unwrap();
        assert!(receipt.is_success() && !receipt.is_failure());
        assert!(matches!(receipt, ExecutionStatus::SuccessReceiptId(_)));

        let failure: ExecutionStatus = serde_json::from_value(serde_json::json!({
            "Failure": {"ActionError": {"index": 0, "kind": {
                "FunctionCallError": {"ExecutionError": "Smart contract panicked"}
            }}}
        }))
        .unwrap();
        assert!(failure.is_failure() && !failure.is_success());
        assert_eq!(failure.failure().and_then(|e| e.index), Some(0));

        assert!(!ExecutionStatus::Unknown.is_success());
        assert!(!ExecutionStatus::Unknown.is_failure());
    }

    #[test]
    fn test_execution_status_failure_invalid_tx_error() {
        let json = serde_json::json!({
            "Failure": {
                "InvalidTxError": "InvalidSignature"
            }
        });
        let status = serde_json::from_value::<ExecutionStatus>(json).unwrap();
        assert!(matches!(
            status,
            ExecutionStatus::InvalidTx(InvalidTxError::InvalidSignature)
        ));
        assert!(status.is_failure() && !status.is_success());
        assert!(status.failure().is_none());
    }

    #[test]
    fn test_global_contract_id_view_formats() {
        // The view format, the pre-2.12 identifier format and the bare string.
        let hash = "9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K";
        for json in [
            serde_json::json!({ "hash": hash }),
            serde_json::json!({ "CodeHash": hash }),
            serde_json::json!(hash),
        ] {
            let id: GlobalContractIdentifierView = serde_json::from_value(json).unwrap();
            assert!(
                matches!(id, GlobalContractIdentifierView::CodeHash(h) if h.to_string() == hash)
            );
        }
        for json in [
            serde_json::json!({ "account_id": "alice.near" }),
            serde_json::json!({ "AccountId": "alice.near" }),
            serde_json::json!("alice.near"),
        ] {
            let id: GlobalContractIdentifierView = serde_json::from_value(json).unwrap();
            assert!(
                matches!(id, GlobalContractIdentifierView::AccountId(a) if a.as_str() == "alice.near")
            );
        }
    }

    #[test]
    fn test_action_view_deterministic_state_init_empty_data() {
        let json = serde_json::json!({
            "DeterministicStateInit": {
                "code": {"account_id": "publisher.near"},
                "deposit": "0"
            }
        });
        let action: ActionView = serde_json::from_value(json).unwrap();
        match action {
            ActionView::DeterministicStateInit { code, data, .. } => {
                assert!(matches!(code, GlobalContractIdentifierView::AccountId(_)));
                assert!(data.is_empty());
            }
            _ => panic!("Expected DeterministicStateInit"),
        }
    }

    #[test]
    fn test_receipt_global_contract_distribution() {
        let json = serde_json::json!({
            "GlobalContractDistribution": {
                "id": {"hash": "9SP8Y3sVADWNN5QoEB5CsvPUE5HT4o8YfBaCnhLss87K"},
                "target_shard": 3,
                "already_delivered_shards": [0, 1, 2],
                "code": "AGFzbQ==",
                "nonce": 42
            }
        });
        let content: ReceiptContent = serde_json::from_value(json).unwrap();
        match content {
            ReceiptContent::GlobalContractDistribution {
                id,
                target_shard,
                already_delivered_shards,
                code,
                nonce,
            } => {
                assert!(matches!(id, GlobalContractIdentifierView::CodeHash(_)));
                assert_eq!(target_shard, 3);
                assert_eq!(already_delivered_shards, vec![0, 1, 2]);
                assert_eq!(code, "AGFzbQ==");
                assert_eq!(nonce, Some(42));
            }
            _ => panic!("Expected GlobalContractDistribution"),
        }
    }

    #[test]
    fn test_receipt_global_contract_distribution_without_nonce() {
        let json = serde_json::json!({
            "GlobalContractDistribution": {
                "id": {"account_id": "publisher.near"},
                "target_shard": 0,
                "already_delivered_shards": [],
                "code": "AGFzbQ=="
            }
        });
        let content: ReceiptContent = serde_json::from_value(json).unwrap();
        match content {
            ReceiptContent::GlobalContractDistribution { nonce, .. } => {
                assert_eq!(nonce, None);
            }
            _ => panic!("Expected GlobalContractDistribution"),
        }
    }

    #[test]
    fn test_transaction_view_nonce_mode() {
        // Nodes older than nearcore 2.12 send no `nonce_mode`.
        for (nonce_mode, expected) in [(None, None), (Some("strict"), Some(NonceMode::Strict))] {
            let mut json = serde_json::json!({
                "signer_id": "alice.near",
                "public_key": PK,
                "nonce": 1,
                "receiver_id": "bob.near",
                "hash": TX_HASH,
                "actions": [{"Transfer": {"deposit": "1000000000000000000000000"}}],
                "signature": SIG
            });
            if let Some(mode) = nonce_mode {
                json["nonce_mode"] = mode.into();
            }
            let tx: TransactionView = serde_json::from_value(json).unwrap();
            assert_eq!(tx.signer_id.as_str(), "alice.near");
            assert_eq!(tx.signature, SIG.parse().unwrap());
            assert_eq!(tx.nonce_mode, expected);
        }
    }

    fn pk() -> PublicKey {
        PK.parse().unwrap()
    }

    fn convert(json: serde_json::Value) -> Result<Action, ActionViewConversionError> {
        let view: ActionView = serde_json::from_value(json).unwrap();
        Action::try_from(view)
    }

    #[test]
    fn function_call_decodes_base64_args() {
        let expected = Action::function_call(
            "set_greeting",
            br#"{"greeting":"hi"}"#.to_vec(),
            Gas::from_tgas(30),
            NearToken::from_near(1),
        );
        let json = serde_json::json!({
            "FunctionCall": {
                "method_name": "set_greeting",
                "args": STANDARD.encode(br#"{"greeting":"hi"}"#),
                "gas": 30000000000000_u64,
                "deposit": "1000000000000000000000000"
            }
        });
        assert_eq!(convert(json).unwrap(), expected);
    }

    #[test]
    fn function_call_rejects_invalid_base64_args() {
        let json = serde_json::json!({
            "FunctionCall": {
                "method_name": "m",
                "args": "not*base64",
                "gas": 1_u64,
                "deposit": "0"
            }
        });
        assert!(matches!(
            convert(json),
            Err(ActionViewConversionError::InvalidBase64 { field: "args", .. })
        ));
    }

    #[test]
    fn universal_state_init_decodes_base64() {
        let json = serde_json::json!({
            "UniversalStateInit": {
                "state_init": "AAAA",
                "deposit": "1000000000000000000000000"
            }
        });
        assert_eq!(
            convert(json).unwrap(),
            Action::UniversalStateInit(UniversalStateInitAction {
                state_init: vec![0, 0, 0],
                deposit: NearToken::from_near(1),
            })
        );
        let json = serde_json::json!({
            "UniversalStateInit": { "state_init": "not base64!", "deposit": "0" }
        });
        assert!(matches!(
            convert(json),
            Err(ActionViewConversionError::InvalidBase64 {
                field: "state_init",
                ..
            })
        ));
    }

    #[test]
    fn simple_actions_round_trip() {
        assert_eq!(
            convert(serde_json::json!("CreateAccount")).unwrap(),
            Action::create_account()
        );
        assert_eq!(
            convert(serde_json::json!({ "Transfer": { "deposit": "5" } })).unwrap(),
            Action::transfer(NearToken::from_yoctonear(5))
        );
        assert_eq!(
            convert(serde_json::json!({ "Stake": { "stake": "7", "public_key": PK } })).unwrap(),
            Action::stake(NearToken::from_yoctonear(7), pk())
        );
        assert_eq!(
            convert(serde_json::json!({ "DeleteKey": { "public_key": PK } })).unwrap(),
            Action::delete_key(pk())
        );
        assert_eq!(
            convert(serde_json::json!({ "DeleteAccount": { "beneficiary_id": "bob.near" } }))
                .unwrap(),
            Action::delete_account("bob.near".parse().unwrap())
        );
        assert_eq!(
            convert(serde_json::json!({
                "TransferToGasKey": { "public_key": PK, "deposit": "3" }
            }))
            .unwrap(),
            Action::transfer_to_gas_key(pk(), NearToken::from_yoctonear(3))
        );
        assert_eq!(
            convert(serde_json::json!({
                "WithdrawFromGasKey": { "public_key": PK, "amount": "4" }
            }))
            .unwrap(),
            Action::withdraw_from_gas_key(pk(), NearToken::from_yoctonear(4))
        );
    }

    #[test]
    fn add_key_function_call_permission_round_trips() {
        let json = serde_json::json!({
            "AddKey": {
                "public_key": PK,
                "access_key": {
                    "nonce": 9_u64,
                    "permission": {
                        "FunctionCall": {
                            "allowance": "250000000000000000000000",
                            "receiver_id": "contract.near",
                            "method_names": ["a", "b"]
                        }
                    }
                }
            }
        });
        let expected = Action::AddKey(AddKeyAction {
            public_key: pk(),
            access_key: AccessKey {
                nonce: 9,
                permission: AccessKeyPermission::FunctionCall(FunctionCallPermission {
                    allowance: Some(NearToken::from_millinear(250)),
                    receiver_id: "contract.near".parse().unwrap(),
                    method_names: vec!["a".into(), "b".into()],
                }),
            },
        });
        assert_eq!(convert(json).unwrap(), expected);
    }

    #[test]
    fn add_key_gas_key_permissions_round_trip() {
        let json = serde_json::json!({
            "AddKey": {
                "public_key": PK,
                "access_key": {
                    "nonce": 0_u64,
                    "permission": {
                        "GasKeyFunctionCall": {
                            "balance": "1000",
                            "num_nonces": 8,
                            "allowance": null,
                            "receiver_id": "contract.near",
                            "method_names": []
                        }
                    }
                }
            }
        });
        let expected = Action::AddKey(AddKeyAction {
            public_key: pk(),
            access_key: AccessKey {
                nonce: 0,
                permission: AccessKeyPermission::GasKeyFunctionCall(
                    GasKeyInfo {
                        balance: NearToken::from_yoctonear(1000),
                        num_nonces: 8,
                    },
                    FunctionCallPermission {
                        allowance: None,
                        receiver_id: "contract.near".parse().unwrap(),
                        method_names: vec![],
                    },
                ),
            },
        });
        assert_eq!(convert(json).unwrap(), expected);

        let full: AccessKeyPermission = serde_json::from_value::<AccessKeyPermissionView>(
            serde_json::json!({ "GasKeyFullAccess": { "balance": "1", "num_nonces": 2 } }),
        )
        .unwrap()
        .into();
        assert_eq!(
            full,
            AccessKeyPermission::GasKeyFullAccess(GasKeyInfo {
                balance: NearToken::from_yoctonear(1),
                num_nonces: 2,
            })
        );
        let full_access: AccessKeyPermission =
            serde_json::from_value::<AccessKeyPermissionView>(serde_json::json!("FullAccess"))
                .unwrap()
                .into();
        assert_eq!(full_access, AccessKeyPermission::FullAccess);
    }

    #[test]
    fn access_key_view_drops_block_metadata() {
        let view: AccessKeyView = serde_json::from_value(serde_json::json!({
            "nonce": 5_u64,
            "permission": "FullAccess",
            "block_height": 1_u64,
            "block_hash": "11111111111111111111111111111111"
        }))
        .unwrap();
        assert_eq!(
            AccessKey::from(view),
            AccessKey {
                nonce: 5,
                permission: AccessKeyPermission::FullAccess
            }
        );
    }

    #[test]
    fn deploy_views_expose_hash_but_cannot_convert_to_actions() {
        let hash = CryptoHash::hash(b"contract code");
        for tag in [
            "DeployContract",
            "DeployGlobalContract",
            "DeployGlobalContractByAccountId",
        ] {
            let json = serde_json::json!({ tag: { "code": STANDARD.encode(hash) } });
            let view: ActionView = serde_json::from_value(json).unwrap();
            assert_eq!(view.deploy_code_hash().unwrap().unwrap(), hash);
            let error = Action::try_from(view).unwrap_err();
            assert_eq!(
                error,
                ActionViewConversionError::DeployCodeUnavailable { code_hash: hash }
            );
            assert_eq!(
                error.to_string(),
                format!(
                    "Cannot convert deploy action view: the node returned only code hash {hash}, not the contract code"
                )
            );
        }
        // Non-deploy views have no code hash.
        let view: ActionView = serde_json::from_value(serde_json::json!("CreateAccount")).unwrap();
        assert!(view.deploy_code_hash().is_none());
    }

    #[test]
    fn deploy_view_with_wrong_length_code_is_rejected() {
        // Someone (or some tool) put actual WASM bytes in `code` instead of the hash.
        let json = serde_json::json!({ "DeployContract": { "code": STANDARD.encode(b"\0asm\x01\0\0\0") } });
        let view: ActionView = serde_json::from_value(json).unwrap();
        assert_eq!(
            view.deploy_code_hash().unwrap(),
            Err(ActionViewConversionError::InvalidCodeHash { actual: 8 })
        );
        assert_eq!(
            Action::try_from(view),
            Err(ActionViewConversionError::InvalidCodeHash { actual: 8 })
        );
    }

    #[test]
    fn use_global_contract_by_hash_and_account() {
        let hash = CryptoHash::hash(b"x");
        assert_eq!(
            convert(serde_json::json!({
                "UseGlobalContract": { "code_hash": hash.to_string() }
            }))
            .unwrap(),
            Action::deploy_from(hash)
        );
        assert_eq!(
            convert(serde_json::json!({
                "UseGlobalContractByAccountId": { "account_id": "publisher.near" }
            }))
            .unwrap(),
            Action::deploy_from("publisher.near".parse::<AccountId>().unwrap())
        );
    }

    #[test]
    fn deterministic_state_init_decodes_base64_data() {
        let hash = CryptoHash::hash(b"y");
        let json = serde_json::json!({
            "DeterministicStateInit": {
                "code": { "hash": hash.to_string() },
                "data": { STANDARD.encode(b"key"): STANDARD.encode(b"value") },
                "deposit": "1"
            }
        });
        let expected = Action::state_init(
            StateInitV1::code(hash)
                .with_data_entry(b"key", b"value")
                .into(),
            NearToken::from_yoctonear(1),
        );
        assert_eq!(convert(json).unwrap(), expected);
    }

    #[test]
    fn delegate_with_nested_function_call_round_trips() {
        let json = serde_json::json!({
            "Delegate": {
                "delegate_action": {
                    "sender_id": "alice.near",
                    "receiver_id": "contract.near",
                    "actions": [{
                        "FunctionCall": {
                            "method_name": "hi",
                            "args": STANDARD.encode(b"{}"),
                            "gas": 5000000000000_u64,
                            "deposit": "0"
                        }
                    }],
                    "nonce": 42_u64,
                    "max_block_height": 100_u64,
                    "public_key": PK
                },
                "signature": SIG
            }
        });
        let inner = Action::function_call(
            "hi",
            b"{}".to_vec(),
            Gas::from_tgas(5),
            NearToken::from_yoctonear(0),
        );
        let expected = Action::Delegate(Box::new(SignedDelegateAction {
            delegate_action: DelegateAction {
                sender_id: "alice.near".parse().unwrap(),
                receiver_id: "contract.near".parse().unwrap(),
                actions: vec![NonDelegateAction::from_action(inner).unwrap()],
                nonce: 42,
                max_block_height: 100,
                public_key: pk(),
            },
            signature: SIG.parse().unwrap(),
        }));
        assert_eq!(convert(json).unwrap(), expected);
    }

    #[test]
    fn delegate_v2_with_gas_key_nonce_round_trips() {
        let json = serde_json::json!({
            "DelegateV2": {
                "delegate_action": {
                    "V2": {
                        "sender_id": "alice.near",
                        "receiver_id": "bob.near",
                        "actions": [{ "Transfer": { "deposit": "1" } }],
                        "nonce": { "GasKeyNonce": { "nonce": 42, "nonce_index": 3 } },
                        "max_block_height": 100,
                        "public_key": PK
                    }
                },
                "signature": SIG
            }
        });
        let expected = Action::DelegateV2(Box::new(VersionedSignedDelegateAction {
            delegate_action: VersionedDelegateActionPayload::V2(DelegateActionV2 {
                sender_id: "alice.near".parse().unwrap(),
                receiver_id: "bob.near".parse().unwrap(),
                actions: vec![
                    NonDelegateAction::from_action(Action::transfer(NearToken::from_yoctonear(1)))
                        .unwrap(),
                ],
                nonce: TransactionNonce::from_nonce_and_index(42, 3),
                max_block_height: 100,
                public_key: pk(),
            }),
            signature: SIG.parse().unwrap(),
        }));
        assert_eq!(convert(json).unwrap(), expected);
    }

    #[test]
    fn nested_delegate_is_rejected() {
        let inner_delegate = serde_json::json!({
            "Delegate": {
                "delegate_action": {
                    "sender_id": "alice.near",
                    "receiver_id": "bob.near",
                    "actions": [],
                    "nonce": 1_u64,
                    "max_block_height": 1_u64,
                    "public_key": PK
                },
                "signature": SIG
            }
        });
        let json = serde_json::json!({
            "Delegate": {
                "delegate_action": {
                    "sender_id": "alice.near",
                    "receiver_id": "bob.near",
                    "actions": [inner_delegate],
                    "nonce": 1_u64,
                    "max_block_height": 1_u64,
                    "public_key": PK
                },
                "signature": SIG
            }
        });
        assert_eq!(
            convert(json),
            Err(ActionViewConversionError::NestedDelegate)
        );
    }
}
