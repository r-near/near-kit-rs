//! Transaction action types.

use std::collections::BTreeMap;
use std::io::Read as _;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use super::{
    AccountId, CryptoHash, Gas, NearToken, PublicKey, Signature, TransactionNonce, TryIntoAccountId,
};

/// NEP-616 deterministic account types, re-exported from the
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts) crate.
///
/// These are the canonical NEP-616 wire types (borsh/JSON byte-identical to nearcore):
///
/// - [`GlobalContractId`] — references a global contract by code hash or publisher account.
/// - [`StateInit`] / [`StateInitV1`] — state initialization for a deterministic account.
///
/// Construct a [`StateInit`] ergonomically via the [`StateInitExt`] extension trait
/// (`StateInit::by_hash(...)` / `StateInit::by_publisher(...)`).
pub use near_global_contracts::{GlobalContractId, StateInit, StateInitV1};

/// Publish mode for global contracts.
///
/// Determines how a published contract will be identified in the global registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishMode {
    /// Contract is identified by the signer's account ID.
    /// The signer can update the contract later.
    Updatable,
    /// Contract is identified by its code hash.
    /// The contract cannot be updated after publishing.
    Immutable,
}

/// Trait for types that can identify a global contract.
///
/// This allows `deploy_from` to accept either a `CryptoHash` (for immutable
/// contracts) or an account ID string/`AccountId` (for publisher-updatable contracts),
/// converting into the canonical [`GlobalContractId`].
///
/// # Panics
///
/// String-based implementations (`&str`, `String`, `&String`) panic if the string is not a
/// valid NEAR account ID.
pub trait IntoGlobalContractId {
    fn into_identifier(self) -> GlobalContractId;
}

impl IntoGlobalContractId for CryptoHash {
    fn into_identifier(self) -> GlobalContractId {
        GlobalContractId::CodeHash(*self.as_bytes())
    }
}

impl IntoGlobalContractId for AccountId {
    fn into_identifier(self) -> GlobalContractId {
        GlobalContractId::AccountId(self)
    }
}

impl IntoGlobalContractId for &AccountId {
    fn into_identifier(self) -> GlobalContractId {
        GlobalContractId::AccountId(self.clone())
    }
}

impl IntoGlobalContractId for &str {
    fn into_identifier(self) -> GlobalContractId {
        let account_id: AccountId = self.try_into_account_id().expect("invalid account ID");
        GlobalContractId::AccountId(account_id)
    }
}

impl IntoGlobalContractId for String {
    fn into_identifier(self) -> GlobalContractId {
        let account_id: AccountId = self.try_into_account_id().expect("invalid account ID");
        GlobalContractId::AccountId(account_id)
    }
}

impl IntoGlobalContractId for &String {
    fn into_identifier(self) -> GlobalContractId {
        let account_id: AccountId = self
            .as_str()
            .try_into_account_id()
            .expect("invalid account ID");
        GlobalContractId::AccountId(account_id)
    }
}

/// NEP-461 domain prefix for V1 delegate actions (meta-transactions, NEP-366).
/// Value: 2^30 + 366 = 1073742190
///
/// This prefix is prepended to a `DelegateAction` when serializing for signing,
/// ensuring delegate action signatures are always distinguishable from regular
/// transaction signatures.
pub const DELEGATE_ACTION_PREFIX: u32 = 1_073_742_190;

/// NEP-461 domain prefix for V2 delegate actions (gas-key meta-transactions,
/// NEP-611). Value: 2^30 + 611 = 1073742435
///
/// This is a **distinct** signing domain from [`DELEGATE_ACTION_PREFIX`]: a V1
/// delegate signature is not valid for a V2 action and vice versa. The prefix is
/// prepended to a borsh-encoded [`VersionedDelegateActionPayload`] (not a bare
/// `DelegateActionV2`) when serializing for signing.
pub const DELEGATE_V2_ACTION_PREFIX: u32 = 1_073_742_435;

/// Gas key information.
///
/// Gas keys are access keys with a prepaid balance to pay for gas costs.
#[derive(
    Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
pub struct GasKeyInfo {
    /// Prepaid gas balance in yoctoNEAR.
    pub balance: NearToken,
    /// Number of nonces allocated for this gas key.
    pub num_nonces: u16,
}

/// Access key permission.
///
/// IMPORTANT: Variant order matters for Borsh serialization!
/// NEAR Protocol defines: 0 = FunctionCall, 1 = FullAccess,
/// 2 = GasKeyFunctionCall, 3 = GasKeyFullAccess
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum AccessKeyPermission {
    /// Function call access with restrictions. (discriminant = 0)
    FunctionCall(FunctionCallPermission),
    /// Full access to the account. (discriminant = 1)
    FullAccess,
    /// Gas key with function call access. (discriminant = 2)
    GasKeyFunctionCall(GasKeyInfo, FunctionCallPermission),
    /// Gas key with full access. (discriminant = 3)
    GasKeyFullAccess(GasKeyInfo),
}

impl AccessKeyPermission {
    /// Create a function call permission.
    pub fn function_call(
        receiver_id: AccountId,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self {
        Self::FunctionCall(FunctionCallPermission {
            allowance,
            receiver_id,
            method_names,
        })
    }

    /// Create a full access permission.
    pub fn full_access() -> Self {
        Self::FullAccess
    }
}

/// Function call access key permission details.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FunctionCallPermission {
    /// Maximum amount this key can spend (None = unlimited).
    pub allowance: Option<NearToken>,
    /// Contract that can be called.
    pub receiver_id: AccountId,
    /// Methods that can be called (empty = all methods).
    pub method_names: Vec<String>,
}

/// Access key attached to an account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct AccessKey {
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Permission level.
    pub permission: AccessKeyPermission,
}

impl AccessKey {
    /// Create a full access key.
    pub fn full_access() -> Self {
        Self {
            nonce: 0,
            permission: AccessKeyPermission::FullAccess,
        }
    }

    /// Create a function call access key.
    pub fn function_call(
        receiver_id: AccountId,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self {
        Self {
            nonce: 0,
            permission: AccessKeyPermission::function_call(receiver_id, method_names, allowance),
        }
    }
}

/// A transaction action.
///
/// IMPORTANT: Variant order matters for Borsh serialization!
/// The discriminants match NEAR Protocol specification:
/// 0 = CreateAccount, 1 = DeployContract, 2 = FunctionCall, 3 = Transfer,
/// 4 = Stake, 5 = AddKey, 6 = DeleteKey, 7 = DeleteAccount, 8 = Delegate,
/// 9 = DeployGlobalContract, 10 = UseGlobalContract, 11 = DeterministicStateInit,
/// 12 = TransferToGasKey, 13 = WithdrawFromGasKey, 14 = DelegateV2
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum Action {
    /// Create a new account. (discriminant = 0)
    CreateAccount(CreateAccountAction),
    /// Deploy contract code. (discriminant = 1)
    DeployContract(DeployContractAction),
    /// Call a contract function. (discriminant = 2)
    FunctionCall(FunctionCallAction),
    /// Transfer NEAR tokens. (discriminant = 3)
    Transfer(TransferAction),
    /// Stake NEAR for validation. (discriminant = 4)
    Stake(StakeAction),
    /// Add an access key. (discriminant = 5)
    AddKey(AddKeyAction),
    /// Delete an access key. (discriminant = 6)
    DeleteKey(DeleteKeyAction),
    /// Delete the account. (discriminant = 7)
    DeleteAccount(DeleteAccountAction),
    /// Delegate action (for meta-transactions). (discriminant = 8)
    Delegate(Box<SignedDelegateAction>),
    /// Publish a contract to global registry. (discriminant = 9)
    DeployGlobalContract(DeployGlobalContractAction),
    /// Deploy from a previously published global contract. (discriminant = 10)
    UseGlobalContract(UseGlobalContractAction),
    /// NEP-616: Deploy with deterministically derived account ID. (discriminant = 11)
    DeterministicStateInit(DeterministicStateInitAction),
    /// Transfer NEAR to a gas key. (discriminant = 12)
    TransferToGasKey(TransferToGasKeyAction),
    /// Withdraw NEAR from a gas key. (discriminant = 13)
    WithdrawFromGasKey(WithdrawFromGasKeyAction),
    /// Gas-key-capable delegate action (meta-transactions, NEP-611). (discriminant = 14)
    DelegateV2(Box<VersionedSignedDelegateAction>),
}

impl Action {
    /// Whether this action is a delegate action of any version (`Delegate` or
    /// `DelegateV2`). Delegate actions must never be nested, so
    /// [`NonDelegateAction`] rejects every variant for which this is `true`.
    pub fn is_delegate(&self) -> bool {
        matches!(self, Action::Delegate(_) | Action::DelegateV2(_))
    }
}

/// Create a new account.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CreateAccountAction;

/// Deploy contract code.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DeployContractAction {
    /// WASM code to deploy.
    pub code: Vec<u8>,
}

/// Call a contract function.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct FunctionCallAction {
    /// Method name to call.
    pub method_name: String,
    /// Arguments (JSON or Borsh encoded).
    pub args: Vec<u8>,
    /// Gas to attach.
    pub gas: Gas,
    /// NEAR tokens to attach.
    pub deposit: NearToken,
}

/// Transfer NEAR tokens.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TransferAction {
    /// Amount to transfer.
    pub deposit: NearToken,
}

/// Stake NEAR for validation.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct StakeAction {
    /// Amount to stake.
    pub stake: NearToken,
    /// Validator public key.
    pub public_key: PublicKey,
}

/// Add an access key.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct AddKeyAction {
    /// Public key to add.
    pub public_key: PublicKey,
    /// Access key details.
    pub access_key: AccessKey,
}

/// Delete an access key.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DeleteKeyAction {
    /// Public key to delete.
    pub public_key: PublicKey,
}

/// Delete the account.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DeleteAccountAction {
    /// Account to receive remaining balance.
    pub beneficiary_id: AccountId,
}

// ============================================================================
// Global Contract Actions
// ============================================================================

/// Deploy mode for global contracts.
///
/// Determines how the contract will be identified in the global registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
#[repr(u8)]
pub enum GlobalContractDeployMode {
    /// Contract is identified by its code hash (immutable).
    /// Other accounts reference it by the hash.
    CodeHash,
    /// Contract is identified by the signer's account ID (updatable).
    /// The signer can update the contract later.
    AccountId,
}

/// Publish a contract to the global registry.
///
/// Global contracts are deployed once and can be referenced by multiple accounts,
/// saving storage costs. The contract can be identified either by its code hash
/// (immutable) or by the publishing account (updatable).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeployGlobalContractAction {
    /// The WASM code to publish.
    pub code: Vec<u8>,
    /// How the contract will be identified.
    pub deploy_mode: GlobalContractDeployMode,
}

/// Deploy a contract from the global registry.
///
/// Instead of uploading the WASM code, this action references a previously
/// published contract in the global registry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct UseGlobalContractAction {
    /// Reference to the published contract.
    pub contract_identifier: GlobalContractId,
}

// ============================================================================
// NEP-616 Deterministic Account Actions
// ============================================================================
//
// The NEP-616 wire types ([`StateInit`], [`StateInitV1`], [`GlobalContractId`]) live in the
// upstream `near-global-contracts` crate and are re-exported above. The action wrapper and the
// `by_hash`/`by_publisher` construction ergonomics stay in near-kit.

/// Deploy a contract with a deterministically derived account ID (NEP-616).
///
/// This enables creating accounts where the account ID is derived from the
/// contract code and initial state, making them predictable and reproducible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeterministicStateInitAction {
    /// The state initialization data.
    pub state_init: StateInit,
    /// Amount to attach for storage costs.
    pub deposit: NearToken,
}

/// Ergonomic constructors for the upstream [`StateInit`] type.
///
/// `near-global-contracts` exposes [`StateInitV1::code`] / [`StateInitV1::with_data_entry`], but
/// near-kit prefers the `by_hash`/`by_publisher` shape. This extension trait restores those
/// constructors so call sites can write `StateInit::by_hash(...)`.
pub trait StateInitExt {
    /// Create a state init referencing a global contract by its code hash (immutable).
    fn by_hash(code_hash: CryptoHash, data: BTreeMap<Vec<u8>, Vec<u8>>) -> StateInit;

    /// Create a state init referencing a global contract by its publisher account (updatable).
    fn by_publisher(publisher_id: AccountId, data: BTreeMap<Vec<u8>, Vec<u8>>) -> StateInit;
}

impl StateInitExt for StateInit {
    fn by_hash(code_hash: CryptoHash, data: BTreeMap<Vec<u8>, Vec<u8>>) -> StateInit {
        StateInit::V1(StateInitV1 {
            code: GlobalContractId::CodeHash(*code_hash.as_bytes()),
            data,
        })
    }

    fn by_publisher(publisher_id: AccountId, data: BTreeMap<Vec<u8>, Vec<u8>>) -> StateInit {
        StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId(publisher_id),
            data,
        })
    }
}

impl DeterministicStateInitAction {
    /// Derive the deterministic account ID for this action.
    ///
    /// Convenience method that delegates to [`StateInit::derive_account_id`].
    pub fn derive_account_id(&self) -> AccountId {
        self.state_init.derive_account_id()
    }
}

/// Transfer NEAR to a gas key.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TransferToGasKeyAction {
    /// Public key of the gas key to fund.
    pub public_key: PublicKey,
    /// Amount of NEAR to transfer.
    pub deposit: NearToken,
}

/// Withdraw NEAR from a gas key.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct WithdrawFromGasKeyAction {
    /// Public key of the gas key to withdraw from.
    pub public_key: PublicKey,
    /// Amount of NEAR to withdraw.
    pub amount: NearToken,
}

/// Delegate action for meta-transactions.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DelegateAction {
    /// Sender of the delegate action.
    pub sender_id: AccountId,
    /// Receiver of the delegate action.
    pub receiver_id: AccountId,
    /// Actions to delegate.
    pub actions: Vec<NonDelegateAction>,
    /// Nonce for replay protection.
    pub nonce: u64,
    /// Maximum block height for the action.
    pub max_block_height: u64,
    /// Public key authorizing the delegate.
    pub public_key: PublicKey,
}

/// Signed delegate action.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct SignedDelegateAction {
    /// The delegate action.
    pub delegate_action: DelegateAction,
    /// Signature over the delegate action.
    pub signature: super::Signature,
}

// ============================================================================
// DelegateV2 (gas-key meta-transactions, NEP-611)
// ============================================================================

/// Delegate action with gas-key support (NEP-611).
///
/// Like the NEP-366 [`DelegateAction`] but its `nonce` is a [`TransactionNonce`]
/// (so it can select one of a gas key's parallel nonces), mirroring
/// [`TransactionV1`](crate::TransactionV1). Carried inside
/// [`VersionedDelegateActionPayload`] and signed under the V2 domain tag.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DelegateActionV2 {
    /// Sender of the delegated actions.
    pub sender_id: AccountId,
    /// Receiver of the delegated actions.
    pub receiver_id: AccountId,
    /// Actions to delegate.
    pub actions: Vec<NonDelegateAction>,
    /// Nonce of the signing key. For a gas key it also selects which parallel
    /// nonce is advanced.
    pub nonce: TransactionNonce,
    /// Maximum block height below which this action is valid.
    pub max_block_height: u64,
    /// Public key used to sign this delegated action.
    pub public_key: PublicKey,
}

/// Versioned payload carried by [`Action::DelegateV2`].
///
/// New delegate versions add a variant here rather than a new `Action` variant.
/// The version tag is part of the signed payload, so a signature can never be
/// ambiguous across versions.
///
/// Borsh discriminant is significant and must match nearcore: `V2 = 0`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum VersionedDelegateActionPayload {
    /// The V2 (gas-key) delegate action. (discriminant = 0)
    V2(DelegateActionV2),
}

impl VersionedDelegateActionPayload {
    /// The public key authorizing the delegate action.
    pub fn public_key(&self) -> &PublicKey {
        match self {
            Self::V2(d) => &d.public_key,
        }
    }

    /// The inner actions as plain [`Action`]s.
    pub fn get_actions(&self) -> Vec<Action> {
        match self {
            Self::V2(d) => d.actions.iter().map(|a| a.clone().into()).collect(),
        }
    }

    /// Serialize this payload for signing under the V2 NEP-461 domain.
    ///
    /// Layout: `DELEGATE_V2_ACTION_PREFIX (u32 LE) ++ borsh(self)`. Because the
    /// prefix differs from [`DELEGATE_ACTION_PREFIX`] and the payload is the
    /// *versioned* enum (so it begins with the `0x00` V2 tag), a V1 delegate
    /// signature is never valid for a V2 action.
    pub fn serialize_for_signing(&self) -> Vec<u8> {
        let prefix_bytes = DELEGATE_V2_ACTION_PREFIX.to_le_bytes();
        let payload_bytes =
            borsh::to_vec(self).expect("delegate action serialization should never fail");

        let mut result = Vec::with_capacity(prefix_bytes.len() + payload_bytes.len());
        result.extend_from_slice(&prefix_bytes);
        result.extend_from_slice(&payload_bytes);
        result
    }

    /// The NEP-461 hash signed over for this payload (the V2 signing domain).
    pub fn get_hash(&self) -> CryptoHash {
        CryptoHash::hash(&self.serialize_for_signing())
    }

    /// Sign this payload, producing a [`VersionedSignedDelegateAction`].
    pub fn sign(self, signature: Signature) -> VersionedSignedDelegateAction {
        VersionedSignedDelegateAction {
            delegate_action: self,
            signature,
        }
    }
}

impl From<DelegateActionV2> for VersionedDelegateActionPayload {
    fn from(d: DelegateActionV2) -> Self {
        Self::V2(d)
    }
}

/// A signed [`VersionedDelegateActionPayload`], carried by [`Action::DelegateV2`].
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct VersionedSignedDelegateAction {
    /// The versioned delegate action payload.
    pub delegate_action: VersionedDelegateActionPayload,
    /// Signature over [`VersionedDelegateActionPayload::get_hash`].
    pub signature: Signature,
}

impl VersionedSignedDelegateAction {
    /// Verify the signature against the payload's public key under the V2 domain.
    pub fn verify(&self) -> bool {
        let hash = self.delegate_action.get_hash();
        self.signature
            .verify(hash.as_bytes(), self.delegate_action.public_key())
    }

    /// Encode to bytes for transport.
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("signed delegate action serialization should never fail")
    }

    /// Encode to base64 for transport.
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.to_bytes())
    }

    /// Decode from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(bytes)
    }

    /// Decode from base64.
    pub fn from_base64(s: &str) -> Result<Self, DecodeError> {
        let bytes = STANDARD.decode(s).map_err(DecodeError::Base64)?;
        Self::from_bytes(&bytes).map_err(DecodeError::Borsh)
    }
}

impl From<VersionedSignedDelegateAction> for Action {
    fn from(action: VersionedSignedDelegateAction) -> Self {
        Self::DelegateV2(Box::new(action))
    }
}

/// Borsh discriminants of the delegate action variants. A `NonDelegateAction`
/// must reject exactly these so a delegate action can never be nested. Must stay
/// in sync with [`Action::is_delegate`]; cross-checked in
/// `test_non_delegate_rejects_all_delegate_variants`.
const DELEGATE_VARIANT_DISCRIMINANTS: [u8; 2] = [8, 14];

/// Non-delegate action (for use within a delegate action).
///
/// This is a newtype wrapper around [`Action`] that ensures the wrapped action
/// is not a delegate variant (`Delegate` or `DelegateV2`), since delegate
/// actions cannot contain nested delegate actions.
///
/// The newtype serializes identically to the inner [`Action`], preserving borsh
/// compatibility with nearcore. Borsh **deserialization** is hand-rolled (rather
/// than derived) so that nested delegate variants are rejected on the wire, not
/// just through the typed constructors — matching nearcore's `NonDelegateAction`.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize)]
pub struct NonDelegateAction(Action);

impl BorshDeserialize for NonDelegateAction {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        let discriminant = u8::deserialize_reader(reader)?;
        if DELEGATE_VARIANT_DISCRIMINANTS.contains(&discriminant) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a delegate action must not contain a nested delegate action",
            ));
        }
        // Re-prepend the discriminant byte and deserialize the full Action.
        // Erase the reader type (`&mut dyn Read`) so chaining doesn't recurse at
        // the type level through `Action::deserialize_reader`'s generic param.
        let prefix = [discriminant];
        let mut chained = prefix.chain(reader);
        let mut reader: &mut dyn std::io::Read = &mut chained;
        Ok(Self(Action::deserialize_reader(&mut reader)?))
    }
}

// Helper constructors for actions
impl Action {
    /// Create a CreateAccount action.
    pub fn create_account() -> Self {
        Self::CreateAccount(CreateAccountAction)
    }

    /// Create a DeployContract action.
    pub fn deploy_contract(code: Vec<u8>) -> Self {
        Self::DeployContract(DeployContractAction { code })
    }

    /// Create a FunctionCall action.
    pub fn function_call(
        method_name: impl Into<String>,
        args: Vec<u8>,
        gas: Gas,
        deposit: NearToken,
    ) -> Self {
        Self::FunctionCall(FunctionCallAction {
            method_name: method_name.into(),
            args,
            gas,
            deposit,
        })
    }

    /// Create a Transfer action.
    pub fn transfer(deposit: NearToken) -> Self {
        Self::Transfer(TransferAction { deposit })
    }

    /// Create a Stake action.
    pub fn stake(stake: NearToken, public_key: PublicKey) -> Self {
        Self::Stake(StakeAction { stake, public_key })
    }

    /// Create an AddKey action for full access.
    pub fn add_full_access_key(public_key: PublicKey) -> Self {
        Self::AddKey(AddKeyAction {
            public_key,
            access_key: AccessKey::full_access(),
        })
    }

    /// Create an AddKey action for function call access.
    pub fn add_function_call_key(
        public_key: PublicKey,
        receiver_id: AccountId,
        method_names: Vec<String>,
        allowance: Option<NearToken>,
    ) -> Self {
        Self::AddKey(AddKeyAction {
            public_key,
            access_key: AccessKey::function_call(receiver_id, method_names, allowance),
        })
    }

    /// Create a DeleteKey action.
    pub fn delete_key(public_key: PublicKey) -> Self {
        Self::DeleteKey(DeleteKeyAction { public_key })
    }

    /// Create a DeleteAccount action.
    pub fn delete_account(beneficiary_id: AccountId) -> Self {
        Self::DeleteAccount(DeleteAccountAction { beneficiary_id })
    }

    /// Create a Delegate action from a signed delegate action.
    pub fn delegate(signed_delegate: SignedDelegateAction) -> Self {
        Self::Delegate(Box::new(signed_delegate))
    }

    /// Create a DelegateV2 action from a versioned signed delegate action
    /// (gas-key meta-transactions, NEP-611).
    pub fn delegate_v2(signed_delegate: VersionedSignedDelegateAction) -> Self {
        Self::DelegateV2(Box::new(signed_delegate))
    }

    /// Publish a contract to the global registry.
    ///
    /// Global contracts are deployed once and can be referenced by multiple accounts,
    /// saving storage costs.
    ///
    /// # Arguments
    ///
    /// * `code` - The WASM code to publish
    /// * `mode` - Whether the contract is updatable (by publisher) or immutable (by hash)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Publish updatable contract (identified by your account)
    /// near.transaction("alice.near")
    ///     .publish(wasm_code, PublishMode::Updatable)
    ///     .send()
    ///     .await?;
    ///
    /// // Publish immutable contract (identified by its hash)
    /// near.transaction("alice.near")
    ///     .publish(wasm_code, PublishMode::Immutable)
    ///     .send()
    ///     .await?;
    /// ```
    pub fn publish(code: Vec<u8>, mode: PublishMode) -> Self {
        Self::DeployGlobalContract(DeployGlobalContractAction {
            code,
            deploy_mode: match mode {
                PublishMode::Updatable => GlobalContractDeployMode::AccountId,
                PublishMode::Immutable => GlobalContractDeployMode::CodeHash,
            },
        })
    }

    /// Deploy a contract from the global registry by code hash.
    ///
    /// References a previously published immutable contract.
    pub fn deploy_from_hash(code_hash: CryptoHash) -> Self {
        Self::UseGlobalContract(UseGlobalContractAction {
            contract_identifier: GlobalContractId::CodeHash(*code_hash.as_bytes()),
        })
    }

    /// Deploy a contract from the global registry by account ID.
    ///
    /// References a contract published by the given account.
    /// The contract can be updated by the publisher.
    pub fn deploy_from_account(account_id: AccountId) -> Self {
        Self::UseGlobalContract(UseGlobalContractAction {
            contract_identifier: GlobalContractId::AccountId(account_id),
        })
    }

    /// Create a NEP-616 deterministic state init action.
    ///
    /// The account ID is derived from the state init data:
    /// `"0s" + hex(keccak256(borsh(state_init))[12..32])`
    pub fn state_init(state_init: StateInit, deposit: NearToken) -> Self {
        Self::DeterministicStateInit(DeterministicStateInitAction {
            state_init,
            deposit,
        })
    }

    /// Transfer NEAR to a gas key.
    pub fn transfer_to_gas_key(public_key: PublicKey, deposit: NearToken) -> Self {
        Self::TransferToGasKey(TransferToGasKeyAction {
            public_key,
            deposit,
        })
    }

    /// Withdraw NEAR from a gas key.
    pub fn withdraw_from_gas_key(public_key: PublicKey, amount: NearToken) -> Self {
        Self::WithdrawFromGasKey(WithdrawFromGasKeyAction { public_key, amount })
    }
}

impl DelegateAction {
    /// Serialize the delegate action for signing.
    ///
    /// Per NEP-461, this prepends a u32 prefix (2^30 + 366) before the delegate action,
    /// ensuring signed delegate actions are never identical to signed transactions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let bytes = delegate_action.serialize_for_signing();
    /// let hash = CryptoHash::hash(&bytes);
    /// let signature = signer.sign(hash.as_bytes()).await?;
    /// ```
    pub fn serialize_for_signing(&self) -> Vec<u8> {
        let prefix_bytes = DELEGATE_ACTION_PREFIX.to_le_bytes();
        let action_bytes =
            borsh::to_vec(self).expect("delegate action serialization should never fail");

        let mut result = Vec::with_capacity(prefix_bytes.len() + action_bytes.len());
        result.extend_from_slice(&prefix_bytes);
        result.extend_from_slice(&action_bytes);
        result
    }

    /// Get the hash of this delegate action (for signing).
    pub fn get_hash(&self) -> CryptoHash {
        let bytes = self.serialize_for_signing();
        CryptoHash::hash(&bytes)
    }

    /// Sign this delegate action and return a SignedDelegateAction.
    pub fn sign(self, signature: Signature) -> SignedDelegateAction {
        SignedDelegateAction {
            delegate_action: self,
            signature,
        }
    }
}

impl SignedDelegateAction {
    /// Encode the signed delegate action to bytes for transport.
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("signed delegate action serialization should never fail")
    }

    /// Encode the signed delegate action to base64 for transport.
    ///
    /// This is the most common format for sending delegate actions via HTTP/JSON.
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.to_bytes())
    }

    /// Decode a signed delegate action from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(bytes)
    }

    /// Decode a signed delegate action from base64.
    pub fn from_base64(s: &str) -> Result<Self, DecodeError> {
        let bytes = STANDARD.decode(s).map_err(DecodeError::Base64)?;
        Self::from_bytes(&bytes).map_err(DecodeError::Borsh)
    }

    /// Get the sender account ID.
    pub fn sender_id(&self) -> &AccountId {
        &self.delegate_action.sender_id
    }

    /// Get the receiver account ID.
    pub fn receiver_id(&self) -> &AccountId {
        &self.delegate_action.receiver_id
    }
}

/// Error decoding a signed delegate action.
#[derive(Debug, thiserror::Error)]
pub enum DecodeError {
    /// Base64 decoding failed.
    #[error("base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
    /// Borsh deserialization failed.
    #[error("borsh decode error: {0}")]
    Borsh(#[from] borsh::io::Error),
}

impl NonDelegateAction {
    /// Convert from an [`Action`], returning `None` if it is a delegate action
    /// of any version (`Delegate` or `DelegateV2`).
    pub fn from_action(action: Action) -> Option<Self> {
        if action.is_delegate() {
            None
        } else {
            Some(Self(action))
        }
    }

    /// Get a reference to the inner action.
    pub fn inner(&self) -> &Action {
        &self.0
    }

    /// Consume self and return the inner action.
    pub fn into_inner(self) -> Action {
        self.0
    }
}

impl From<NonDelegateAction> for Action {
    fn from(action: NonDelegateAction) -> Self {
        action.0
    }
}

impl TryFrom<Action> for NonDelegateAction {
    type Error = ();

    fn try_from(action: Action) -> Result<Self, Self::Error> {
        Self::from_action(action).ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Gas, NearToken, SecretKey};

    fn create_test_delegate_action() -> DelegateAction {
        let sender_id: AccountId = "alice.testnet".parse().unwrap();
        let receiver_id: AccountId = "bob.testnet".parse().unwrap();
        let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();

        DelegateAction {
            sender_id,
            receiver_id,
            actions: vec![
                NonDelegateAction::from_action(Action::Transfer(TransferAction {
                    deposit: NearToken::from_near(1),
                }))
                .unwrap(),
            ],
            nonce: 1,
            max_block_height: 1000,
            public_key,
        }
    }

    #[test]
    fn test_delegate_action_prefix() {
        // NEP-461: prefix = 2^30 + 366
        assert_eq!(DELEGATE_ACTION_PREFIX, 1073742190);
        assert_eq!(DELEGATE_ACTION_PREFIX, (1 << 30) + 366);
    }

    #[test]
    fn test_delegate_action_serialize_for_signing() {
        let delegate_action = create_test_delegate_action();
        let bytes = delegate_action.serialize_for_signing();

        // First 4 bytes should be the NEP-461 prefix in little-endian
        let prefix_bytes = &bytes[0..4];
        let prefix = u32::from_le_bytes(prefix_bytes.try_into().unwrap());
        assert_eq!(prefix, DELEGATE_ACTION_PREFIX);

        // Rest should be borsh-serialized DelegateAction
        let action_bytes = &bytes[4..];
        let expected_action_bytes = borsh::to_vec(&delegate_action).unwrap();
        assert_eq!(action_bytes, expected_action_bytes.as_slice());
    }

    #[test]
    fn test_delegate_action_get_hash() {
        let delegate_action = create_test_delegate_action();
        let hash = delegate_action.get_hash();

        // Hash should be SHA-256 of serialize_for_signing bytes
        let bytes = delegate_action.serialize_for_signing();
        let expected_hash = CryptoHash::hash(&bytes);
        assert_eq!(hash, expected_hash);
    }

    #[test]
    fn test_signed_delegate_action_roundtrip_bytes() {
        let delegate_action = create_test_delegate_action();
        let secret_key = SecretKey::generate_ed25519();
        let hash = delegate_action.get_hash();
        let signature = secret_key.sign(hash.as_bytes());
        let signed = delegate_action.sign(signature);

        // Roundtrip through bytes
        let bytes = signed.to_bytes();
        let decoded = SignedDelegateAction::from_bytes(&bytes).unwrap();

        assert_eq!(decoded.sender_id().as_str(), signed.sender_id().as_str());
        assert_eq!(
            decoded.receiver_id().as_str(),
            signed.receiver_id().as_str()
        );
        assert_eq!(decoded.delegate_action.nonce, signed.delegate_action.nonce);
        assert_eq!(
            decoded.delegate_action.max_block_height,
            signed.delegate_action.max_block_height
        );
    }

    #[test]
    fn test_signed_delegate_action_roundtrip_base64() {
        let delegate_action = create_test_delegate_action();
        let secret_key = SecretKey::generate_ed25519();
        let hash = delegate_action.get_hash();
        let signature = secret_key.sign(hash.as_bytes());
        let signed = delegate_action.sign(signature);

        // Roundtrip through base64
        let base64 = signed.to_base64();
        let decoded = SignedDelegateAction::from_base64(&base64).unwrap();

        assert_eq!(decoded.sender_id().as_str(), signed.sender_id().as_str());
        assert_eq!(
            decoded.receiver_id().as_str(),
            signed.receiver_id().as_str()
        );
    }

    #[test]
    fn test_signed_delegate_action_accessors() {
        let delegate_action = create_test_delegate_action();
        let secret_key = SecretKey::generate_ed25519();
        let hash = delegate_action.get_hash();
        let signature = secret_key.sign(hash.as_bytes());
        let signed = delegate_action.sign(signature);

        assert_eq!(signed.sender_id().as_str(), "alice.testnet");
        assert_eq!(signed.receiver_id().as_str(), "bob.testnet");
    }

    #[test]
    fn test_non_delegate_action_from_action() {
        // Transfer should convert
        let transfer = Action::Transfer(TransferAction {
            deposit: NearToken::from_near(1),
        });
        assert!(NonDelegateAction::from_action(transfer).is_some());

        // FunctionCall should convert
        let call = Action::FunctionCall(FunctionCallAction {
            method_name: "test".to_string(),
            args: vec![],
            gas: Gas::default(),
            deposit: NearToken::ZERO,
        });
        assert!(NonDelegateAction::from_action(call).is_some());

        // Delegate should NOT convert (returns None)
        let delegate_action = create_test_delegate_action();
        let secret_key = SecretKey::generate_ed25519();
        let hash = delegate_action.get_hash();
        let signature = secret_key.sign(hash.as_bytes());
        let signed = delegate_action.sign(signature);
        let delegate = Action::delegate(signed);
        assert!(NonDelegateAction::from_action(delegate).is_none());
    }

    #[test]
    fn test_decode_error_display() {
        // Test that DecodeError has proper Display impl
        let base64_err = DecodeError::Base64(base64::DecodeError::InvalidLength(5));
        assert!(format!("{}", base64_err).contains("base64"));

        // Borsh error is harder to construct, but we tested the variant exists
    }

    // ========================================================================
    // Global Contract Action Tests
    // ========================================================================

    #[test]
    fn test_action_discriminants() {
        // Verify action discriminants match NEAR protocol specification
        // 0 = CreateAccount, 1 = DeployContract, 2 = FunctionCall, 3 = Transfer,
        // 4 = Stake, 5 = AddKey, 6 = DeleteKey, 7 = DeleteAccount, 8 = Delegate,
        // 9 = DeployGlobalContract, 10 = UseGlobalContract, 11 = DeterministicStateInit,
        // 12 = TransferToGasKey, 13 = WithdrawFromGasKey

        let create_account = Action::create_account();
        let bytes = borsh::to_vec(&create_account).unwrap();
        assert_eq!(bytes[0], 0, "CreateAccount should have discriminant 0");

        let deploy = Action::deploy_contract(vec![1, 2, 3]);
        let bytes = borsh::to_vec(&deploy).unwrap();
        assert_eq!(bytes[0], 1, "DeployContract should have discriminant 1");

        let transfer = Action::transfer(NearToken::from_near(1));
        let bytes = borsh::to_vec(&transfer).unwrap();
        assert_eq!(bytes[0], 3, "Transfer should have discriminant 3");

        // DeployGlobalContract (discriminant = 9)
        let publish = Action::publish(vec![1, 2, 3], PublishMode::Updatable);
        let bytes = borsh::to_vec(&publish).unwrap();
        assert_eq!(
            bytes[0], 9,
            "DeployGlobalContract should have discriminant 9"
        );

        // UseGlobalContract (discriminant = 10)
        let code_hash = CryptoHash::hash(&[1, 2, 3]);
        let use_global = Action::deploy_from_hash(code_hash);
        let bytes = borsh::to_vec(&use_global).unwrap();
        assert_eq!(
            bytes[0], 10,
            "UseGlobalContract should have discriminant 10"
        );

        // DeterministicStateInit (discriminant = 11)
        let state_init = Action::state_init(
            StateInit::by_hash(code_hash, BTreeMap::new()),
            NearToken::from_near(1),
        );
        let bytes = borsh::to_vec(&state_init).unwrap();
        assert_eq!(
            bytes[0], 11,
            "DeterministicStateInit should have discriminant 11"
        );

        // TransferToGasKey (discriminant = 12)
        let pk: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let transfer_gas = Action::transfer_to_gas_key(pk.clone(), NearToken::from_near(1));
        let bytes = borsh::to_vec(&transfer_gas).unwrap();
        assert_eq!(bytes[0], 12, "TransferToGasKey should have discriminant 12");

        // WithdrawFromGasKey (discriminant = 13)
        let withdraw_gas = Action::withdraw_from_gas_key(pk, NearToken::from_near(1));
        let bytes = borsh::to_vec(&withdraw_gas).unwrap();
        assert_eq!(
            bytes[0], 13,
            "WithdrawFromGasKey should have discriminant 13"
        );
    }

    #[test]
    fn test_global_contract_deploy_mode_serialization() {
        // Verify deploy mode serialization
        let by_hash = GlobalContractDeployMode::CodeHash;
        let bytes = borsh::to_vec(&by_hash).unwrap();
        assert_eq!(bytes, vec![0], "CodeHash mode should serialize to 0");

        let by_account = GlobalContractDeployMode::AccountId;
        let bytes = borsh::to_vec(&by_account).unwrap();
        assert_eq!(bytes, vec![1], "AccountId mode should serialize to 1");
    }

    #[test]
    fn test_global_contract_identifier_serialization() {
        // Verify identifier serialization
        let hash = CryptoHash::hash(&[1, 2, 3]);
        let by_hash = GlobalContractId::CodeHash(*hash.as_bytes());
        let bytes = borsh::to_vec(&by_hash).unwrap();
        assert_eq!(
            bytes[0], 0,
            "CodeHash identifier should have discriminant 0"
        );
        assert_eq!(
            bytes.len(),
            1 + 32,
            "Should be 1 byte discriminant + 32 byte hash"
        );

        let account_id: AccountId = "test.near".parse().unwrap();
        let by_account = GlobalContractId::AccountId(account_id);
        let bytes = borsh::to_vec(&by_account).unwrap();
        assert_eq!(
            bytes[0], 1,
            "AccountId identifier should have discriminant 1"
        );
    }

    #[test]
    fn test_deploy_global_contract_action_roundtrip() {
        let code = vec![0, 97, 115, 109]; // WASM magic bytes
        let action = DeployGlobalContractAction {
            code: code.clone(),
            deploy_mode: GlobalContractDeployMode::CodeHash,
        };

        let bytes = borsh::to_vec(&action).unwrap();
        let decoded: DeployGlobalContractAction = borsh::from_slice(&bytes).unwrap();

        assert_eq!(decoded.code, code);
        assert_eq!(decoded.deploy_mode, GlobalContractDeployMode::CodeHash);
    }

    #[test]
    fn test_use_global_contract_action_roundtrip() {
        let hash = CryptoHash::hash(&[1, 2, 3, 4]);
        let action = UseGlobalContractAction {
            contract_identifier: GlobalContractId::CodeHash(*hash.as_bytes()),
        };

        let bytes = borsh::to_vec(&action).unwrap();
        let decoded: UseGlobalContractAction = borsh::from_slice(&bytes).unwrap();

        assert_eq!(
            decoded.contract_identifier,
            GlobalContractId::CodeHash(*hash.as_bytes())
        );
    }

    #[test]
    fn test_deterministic_state_init_roundtrip() {
        let hash = CryptoHash::hash(&[1, 2, 3, 4]);
        let mut data = BTreeMap::new();
        data.insert(b"key1".to_vec(), b"value1".to_vec());
        data.insert(b"key2".to_vec(), b"value2".to_vec());

        let action = DeterministicStateInitAction {
            state_init: StateInit::V1(StateInitV1 {
                code: GlobalContractId::CodeHash(*hash.as_bytes()),
                data: data.clone(),
            }),
            deposit: NearToken::from_near(5),
        };

        let bytes = borsh::to_vec(&action).unwrap();
        let decoded: DeterministicStateInitAction = borsh::from_slice(&bytes).unwrap();

        assert_eq!(decoded.deposit, NearToken::from_near(5));
        let StateInit::V1(v1) = decoded.state_init;
        assert_eq!(v1.code, GlobalContractId::CodeHash(*hash.as_bytes()));
        assert_eq!(v1.data, data);
    }

    #[test]
    fn test_action_helper_constructors() {
        // Test publish
        let code = vec![1, 2, 3];
        let action = Action::publish(code.clone(), PublishMode::Immutable);
        if let Action::DeployGlobalContract(inner) = action {
            assert_eq!(inner.code, code);
            assert_eq!(inner.deploy_mode, GlobalContractDeployMode::CodeHash);
        } else {
            panic!("Expected DeployGlobalContract");
        }

        let action = Action::publish(code.clone(), PublishMode::Updatable);
        if let Action::DeployGlobalContract(inner) = action {
            assert_eq!(inner.deploy_mode, GlobalContractDeployMode::AccountId);
        } else {
            panic!("Expected DeployGlobalContract");
        }

        // Test deploy_from_hash
        let hash = CryptoHash::hash(&code);
        let action = Action::deploy_from_hash(hash);
        if let Action::UseGlobalContract(inner) = action {
            assert_eq!(
                inner.contract_identifier,
                GlobalContractId::CodeHash(*hash.as_bytes())
            );
        } else {
            panic!("Expected UseGlobalContract");
        }

        // Test deploy_from_account
        let account_id: AccountId = "publisher.near".parse().unwrap();
        let action = Action::deploy_from_account(account_id.clone());
        if let Action::UseGlobalContract(inner) = action {
            assert_eq!(
                inner.contract_identifier,
                GlobalContractId::AccountId(account_id)
            );
        } else {
            panic!("Expected UseGlobalContract");
        }
    }

    #[test]
    fn test_derive_account_id_format() {
        // Test that derived account ID has the correct format
        let state_init = StateInit::V1(StateInitV1 {
            code: GlobalContractId::CodeHash([0u8; 32]),
            data: BTreeMap::new(),
        });

        let account_id = state_init.derive_account_id();
        let account_str = account_id.as_str();

        // Should start with "0s"
        assert!(
            account_str.starts_with("0s"),
            "Derived account should start with '0s', got: {}",
            account_str
        );

        // Should be exactly 42 characters: "0s" + 40 hex chars
        assert_eq!(
            account_str.len(),
            42,
            "Derived account should be 42 chars, got: {}",
            account_str.len()
        );

        // Everything after "0s" should be valid lowercase hex
        let hex_part = &account_str[2..];
        assert!(
            hex_part
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "Hex part should be lowercase hex, got: {}",
            hex_part
        );
    }

    #[test]
    fn test_derive_account_id_deterministic() {
        // Same input should produce same output
        let state_init1 = StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId("publisher.near".parse().unwrap()),
            data: BTreeMap::new(),
        });

        let state_init2 = StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId("publisher.near".parse().unwrap()),
            data: BTreeMap::new(),
        });

        assert_eq!(
            state_init1.derive_account_id(),
            state_init2.derive_account_id(),
            "Same input should produce same account ID"
        );
    }

    #[test]
    fn test_derive_account_id_different_inputs() {
        // Different code references should produce different account IDs
        let state_init1 = StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId("publisher1.near".parse().unwrap()),
            data: BTreeMap::new(),
        });

        let state_init2 = StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId("publisher2.near".parse().unwrap()),
            data: BTreeMap::new(),
        });

        assert_ne!(
            state_init1.derive_account_id(),
            state_init2.derive_account_id(),
            "Different code references should produce different account IDs"
        );
    }

    #[test]
    fn test_access_key_permission_discriminants() {
        let fc = AccessKeyPermission::FunctionCall(FunctionCallPermission {
            allowance: None,
            receiver_id: "test.near".parse().unwrap(),
            method_names: vec![],
        });
        let bytes = borsh::to_vec(&fc).unwrap();
        assert_eq!(bytes[0], 0, "FunctionCall should have discriminant 0");

        let fa = AccessKeyPermission::FullAccess;
        let bytes = borsh::to_vec(&fa).unwrap();
        assert_eq!(bytes[0], 1, "FullAccess should have discriminant 1");

        let gkfc = AccessKeyPermission::GasKeyFunctionCall(
            GasKeyInfo {
                balance: NearToken::from_near(1),
                num_nonces: 5,
            },
            FunctionCallPermission {
                allowance: None,
                receiver_id: "test.near".parse().unwrap(),
                method_names: vec![],
            },
        );
        let bytes = borsh::to_vec(&gkfc).unwrap();
        assert_eq!(bytes[0], 2, "GasKeyFunctionCall should have discriminant 2");

        let gkfa = AccessKeyPermission::GasKeyFullAccess(GasKeyInfo {
            balance: NearToken::from_near(1),
            num_nonces: 5,
        });
        let bytes = borsh::to_vec(&gkfa).unwrap();
        assert_eq!(bytes[0], 3, "GasKeyFullAccess should have discriminant 3");
    }

    #[test]
    fn test_derive_account_id_different_data() {
        // Different data should produce different account IDs
        let mut data = BTreeMap::new();
        data.insert(b"key".to_vec(), b"value".to_vec());

        let state_init1 = StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId("publisher.near".parse().unwrap()),
            data: BTreeMap::new(),
        });

        let state_init2 = StateInit::V1(StateInitV1 {
            code: GlobalContractId::AccountId("publisher.near".parse().unwrap()),
            data,
        });

        assert_ne!(
            state_init1.derive_account_id(),
            state_init2.derive_account_id(),
            "Different data should produce different account IDs"
        );
    }

    // ========================================================================
    // Deterministic Types JSON Serialization Tests
    // ========================================================================

    #[test]
    fn test_deterministic_state_init_json_roundtrip() {
        // Build a StateInit with non-trivial data
        let hash = CryptoHash::hash(&[1, 2, 3, 4]);
        let mut data = BTreeMap::new();
        data.insert(b"key1".to_vec(), b"value1".to_vec());
        data.insert(b"key2".to_vec(), b"value2".to_vec());

        let state_init = StateInit::V1(StateInitV1 {
            code: GlobalContractId::CodeHash(*hash.as_bytes()),
            data: data.clone(),
        });

        // Serialize to JSON
        let json = serde_json::to_value(&state_init).unwrap();

        // Verify externally-tagged format: {"V1": {...}} (matching nearcore)
        assert!(
            json.get("V1").is_some(),
            "Expected externally-tagged 'V1' key, got: {json}"
        );
        let v1 = json.get("V1").unwrap();
        assert!(v1.get("code").is_some(), "Expected 'code' field in V1");
        assert!(v1.get("data").is_some(), "Expected 'data' field in V1");

        // Verify data keys/values are base64-encoded
        let data_obj = v1.get("data").unwrap().as_object().unwrap();
        // "key1" in base64 is "a2V5MQ=="
        assert!(
            data_obj.contains_key("a2V5MQ=="),
            "Expected base64-encoded key 'a2V5MQ==', got keys: {:?}",
            data_obj.keys().collect::<Vec<_>>()
        );

        // Round-trip back
        let deserialized: StateInit = serde_json::from_value(json).unwrap();
        let StateInit::V1(v1_decoded) = deserialized;
        assert_eq!(
            v1_decoded.code,
            GlobalContractId::CodeHash(*hash.as_bytes())
        );
        assert_eq!(v1_decoded.data, data);
    }

    #[test]
    fn test_global_contract_identifier_json_roundtrip() {
        // CodeHash variant
        let hash = CryptoHash::hash(&[1, 2, 3]);
        let id = GlobalContractId::CodeHash(*hash.as_bytes());
        let json = serde_json::to_string(&id).unwrap();
        let decoded: GlobalContractId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);

        // AccountId variant
        let account_id: AccountId = "test.near".parse().unwrap();
        let id = GlobalContractId::AccountId(account_id);
        let json = serde_json::to_string(&id).unwrap();
        let decoded: GlobalContractId = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, id);
    }

    #[test]
    fn test_deterministic_state_init_action_json_roundtrip() {
        let action = DeterministicStateInitAction {
            state_init: StateInit::V1(StateInitV1 {
                code: GlobalContractId::AccountId("publisher.near".parse().unwrap()),
                data: BTreeMap::new(),
            }),
            deposit: NearToken::from_near(5),
        };

        let json = serde_json::to_string(&action).unwrap();
        let decoded: DeterministicStateInitAction = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, action);
    }

    // ========================================================================
    // DelegateV2 tests
    // ========================================================================

    fn sample_delegate_v2(public_key: PublicKey) -> DelegateActionV2 {
        DelegateActionV2 {
            sender_id: "alice.testnet".parse().unwrap(),
            receiver_id: "bob.testnet".parse().unwrap(),
            actions: vec![
                NonDelegateAction::from_action(Action::Transfer(TransferAction {
                    deposit: NearToken::from_near(1),
                }))
                .unwrap(),
            ],
            nonce: TransactionNonce::from_nonce_and_index(7, 3),
            max_block_height: 1000,
            public_key,
        }
    }

    /// The V2 domain prefix must be NEP-611 (2^30 + 611) and distinct from V1.
    #[test]
    fn test_delegate_v2_prefix() {
        assert_eq!(DELEGATE_V2_ACTION_PREFIX, 1073742435);
        assert_eq!(DELEGATE_V2_ACTION_PREFIX, (1 << 30) + 611);
        assert_ne!(DELEGATE_V2_ACTION_PREFIX, DELEGATE_ACTION_PREFIX);
    }

    /// `Action::DelegateV2` must have borsh discriminant 14, and round-trip.
    #[test]
    fn test_delegate_v2_action_discriminant_and_roundtrip() {
        let secret = SecretKey::generate_ed25519();
        let payload: VersionedDelegateActionPayload =
            sample_delegate_v2(secret.public_key()).into();
        let sig = secret.sign(payload.get_hash().as_bytes());
        let action = Action::delegate_v2(payload.sign(sig));

        let bytes = borsh::to_vec(&action).unwrap();
        assert_eq!(bytes[0], 14, "DelegateV2 must have discriminant 14");

        let decoded: Action = borsh::from_slice(&bytes).unwrap();
        assert_eq!(decoded, action);
    }

    /// `VersionedDelegateActionPayload::V2` must have borsh discriminant 0, and
    /// the signing bytes must be `prefix_le ++ [0x00] ++ borsh(DelegateActionV2)`.
    #[test]
    fn test_versioned_payload_discriminant_and_signing_layout() {
        let pk: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp"
            .parse()
            .unwrap();
        let v2 = sample_delegate_v2(pk);
        let payload: VersionedDelegateActionPayload = v2.clone().into();

        let payload_bytes = borsh::to_vec(&payload).unwrap();
        assert_eq!(
            payload_bytes[0], 0,
            "VersionedDelegateActionPayload::V2 must be discriminant 0"
        );

        let signing = payload.serialize_for_signing();
        assert_eq!(
            u32::from_le_bytes(signing[0..4].try_into().unwrap()),
            DELEGATE_V2_ACTION_PREFIX
        );
        assert_eq!(&signing[4..], payload_bytes.as_slice());
    }

    /// Sign + verify happy path for a V2 delegate action.
    #[test]
    fn test_delegate_v2_sign_verify() {
        let secret = SecretKey::generate_ed25519();
        let payload: VersionedDelegateActionPayload =
            sample_delegate_v2(secret.public_key()).into();
        let sig = secret.sign(payload.get_hash().as_bytes());
        let signed = payload.sign(sig);
        assert!(signed.verify());

        // Round-trips through bytes and base64.
        let back = VersionedSignedDelegateAction::from_bytes(&signed.to_bytes()).unwrap();
        assert_eq!(back, signed);
        let back64 = VersionedSignedDelegateAction::from_base64(&signed.to_base64()).unwrap();
        assert_eq!(back64, signed);
    }

    /// A signature bound to one gas-key nonce index must not verify for another:
    /// the nonce is part of the signed payload.
    #[test]
    fn test_delegate_v2_nonce_index_bound_into_signature() {
        let secret = SecretKey::generate_ed25519();
        let v2 = sample_delegate_v2(secret.public_key()); // nonce_index 3
        let payload: VersionedDelegateActionPayload = v2.clone().into();
        let signed = payload
            .clone()
            .sign(secret.sign(payload.get_hash().as_bytes()));

        let mut tampered_v2 = v2;
        tampered_v2.nonce = TransactionNonce::from_nonce_and_index(7, 4); // different index
        let forged = VersionedSignedDelegateAction {
            delegate_action: tampered_v2.into(),
            signature: signed.signature.clone(),
        };
        assert!(
            !forged.verify(),
            "signature must not verify for a different nonce index"
        );
    }

    /// The V1 and V2 signing domains are disjoint: a signature produced under the
    /// V1 (NEP-366) prefix must NOT verify a V2 action, and vice versa.
    #[test]
    fn test_v1_and_v2_signing_domains_disjoint() {
        let secret = SecretKey::generate_ed25519();
        let pk = secret.public_key();
        let v2 = sample_delegate_v2(pk.clone());
        let payload: VersionedDelegateActionPayload = v2.into();

        // Forge a signature over the *V1* prefix + the V2 payload bytes.
        let mut v1_domain_bytes = DELEGATE_ACTION_PREFIX.to_le_bytes().to_vec();
        v1_domain_bytes.extend_from_slice(&borsh::to_vec(&payload).unwrap());
        let v1_sig = secret.sign(CryptoHash::hash(&v1_domain_bytes).as_bytes());

        let forged = VersionedSignedDelegateAction {
            delegate_action: payload.clone(),
            signature: v1_sig,
        };
        assert!(
            !forged.verify(),
            "a V1-domain signature must not verify a V2 action"
        );

        // And the correct V2 signature obviously does verify.
        let v2_sig = secret.sign(payload.get_hash().as_bytes());
        let valid = VersionedSignedDelegateAction {
            delegate_action: payload,
            signature: v2_sig,
        };
        assert!(valid.verify());
    }

    /// `Action::is_delegate` must be true for both delegate variants and false
    /// for everything else. Kept in sync with `DELEGATE_VARIANT_DISCRIMINANTS`.
    #[test]
    fn test_is_delegate() {
        let secret = SecretKey::generate_ed25519();
        let v1 = Action::delegate(
            create_test_delegate_action()
                .sign(secret.sign(create_test_delegate_action().get_hash().as_bytes())),
        );
        let payload: VersionedDelegateActionPayload =
            sample_delegate_v2(secret.public_key()).into();
        let v2 = Action::delegate_v2(
            payload
                .clone()
                .sign(secret.sign(payload.get_hash().as_bytes())),
        );

        assert!(v1.is_delegate());
        assert!(v2.is_delegate());
        assert!(!Action::transfer(NearToken::from_near(1)).is_delegate());

        // Cross-check the borsh discriminants match DELEGATE_VARIANT_DISCRIMINANTS.
        assert_eq!(
            borsh::to_vec(&v1).unwrap()[0],
            DELEGATE_VARIANT_DISCRIMINANTS[0]
        );
        assert_eq!(
            borsh::to_vec(&v2).unwrap()[0],
            DELEGATE_VARIANT_DISCRIMINANTS[1]
        );
    }

    /// `NonDelegateAction` must reject BOTH delegate variants, via the typed
    /// constructor AND on the wire (borsh), matching nearcore.
    #[test]
    fn test_non_delegate_rejects_all_delegate_variants() {
        let secret = SecretKey::generate_ed25519();

        let v1_signed = create_test_delegate_action()
            .sign(secret.sign(create_test_delegate_action().get_hash().as_bytes()));
        let v1 = Action::delegate(v1_signed);

        let payload: VersionedDelegateActionPayload =
            sample_delegate_v2(secret.public_key()).into();
        let v2 = Action::delegate_v2(
            payload
                .clone()
                .sign(secret.sign(payload.get_hash().as_bytes())),
        );

        for action in [v1, v2] {
            // Typed path rejects.
            assert!(NonDelegateAction::from_action(action.clone()).is_none());
            assert!(NonDelegateAction::try_from(action.clone()).is_err());

            // Borsh path rejects (the bytes of a delegate action embedded where a
            // NonDelegateAction is expected).
            let bytes = borsh::to_vec(&action).unwrap();
            let err = NonDelegateAction::try_from_slice(&bytes).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }

        // A non-delegate action is accepted on both paths.
        let transfer = Action::transfer(NearToken::from_near(1));
        assert!(NonDelegateAction::from_action(transfer.clone()).is_some());
        let bytes = borsh::to_vec(&transfer).unwrap();
        assert!(NonDelegateAction::try_from_slice(&bytes).is_ok());
    }
}
