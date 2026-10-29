//! Transaction action types.

use base64::{Engine as _, engine::general_purpose::STANDARD};
use borsh::de::EnumExt as _;
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

use crate::error::StateInitError;

use super::{
    AccountId, CryptoHash, Gas, NearToken, PublicKey, RawStateInit, Signature, TransactionNonce,
    UniversalStateInit,
};

/// NEP-616 deterministic account types, re-exported from the
/// [`near-global-contracts`](https://crates.io/crates/near-global-contracts) crate.
///
/// These are the canonical NEP-616 wire types (borsh/JSON byte-identical to nearcore):
///
/// - [`GlobalContractId`] — references a global contract by code hash or publisher account.
/// - [`StateInit`] / [`StateInitV1`] — state initialization for a deterministic account.
///
/// Build a [`StateInit`] from a [`StateInitV1`]:
/// `StateInitV1::code(code_hash_or_publisher).with_data_entry(key, value).into()`.
pub use near_global_contracts::{GlobalContractId, StateInit, StateInitV1};

/// Publish mode for global contracts.
///
/// Determines how a published contract will be identified in the global registry.
/// The user-facing names map directly onto nearcore's wire names:
/// `Immutable` is encoded as `CodeHash` (`0`) and `Updatable` as `AccountId` (`1`).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize,
)]
#[repr(u8)]
pub enum PublishMode {
    /// Contract is identified by its code hash.
    /// The contract cannot be updated after publishing.
    #[serde(rename = "CodeHash")]
    Immutable,
    /// Contract is identified by the signer's account ID.
    /// The signer can update the contract later.
    #[serde(rename = "AccountId")]
    Updatable,
}

/// NEP-461 domain prefix for V1 delegate actions (meta-transactions, NEP-366):
/// 2^30 + 366.
///
/// This prefix is prepended to a `DelegateAction` when serializing for signing,
/// ensuring delegate action signatures are always distinguishable from regular
/// transaction signatures.
pub const DELEGATE_ACTION_PREFIX: u32 = 1_073_742_190;

/// NEP-461 domain prefix for V2 delegate actions (gas-key meta-transactions,
/// NEP-611): 2^30 + 611.
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
/// Variant order is the borsh tag; never reorder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum AccessKeyPermission {
    /// Function call access with restrictions.
    FunctionCall(FunctionCallPermission),
    /// Full access to the account.
    FullAccess,
    /// Gas key with function call access.
    GasKeyFunctionCall(GasKeyInfo, FunctionCallPermission),
    /// Gas key with full access.
    GasKeyFullAccess(GasKeyInfo),
}

impl AccessKeyPermission {
    /// Maximum number of parallel nonces that a gas key may allocate.
    pub const MAX_NONCES_FOR_GAS_KEY: u16 = 1024;
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
}

/// A transaction action.
///
/// Variant order is the borsh tag (nearcore's); never reorder.
///
/// The serde (JSON) form is nearcore's protocol `Action` JSON, externally
/// tagged by variant name: `{"CreateAccount": {}}`,
/// `{"Transfer": {"deposit": "1"}}`, `{"DeployContract": {"code": "<base64 WASM>"}}`.
/// The node uses this form for the inner actions of a delegate action in RPC
/// responses ([`DelegateActionView::actions`](crate::rpc::DelegateActionView::actions)).
/// It is not the [`ActionView`](crate::rpc::ActionView) shape the node uses
/// for a transaction's top-level actions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum Action {
    /// Create a new account.
    CreateAccount(CreateAccountAction),
    /// Deploy contract code.
    DeployContract(DeployContractAction),
    /// Call a contract function.
    FunctionCall(FunctionCallAction),
    /// Transfer NEAR tokens.
    Transfer(TransferAction),
    /// Stake NEAR for validation.
    Stake(StakeAction),
    /// Add an access key.
    AddKey(AddKeyAction),
    /// Delete an access key.
    DeleteKey(DeleteKeyAction),
    /// Delete the account.
    DeleteAccount(DeleteAccountAction),
    /// Delegate action (for meta-transactions).
    Delegate(Box<SignedDelegateAction>),
    /// Publish a contract to global registry.
    DeployGlobalContract(DeployGlobalContractAction),
    /// Deploy from a previously published global contract.
    UseGlobalContract(UseGlobalContractAction),
    /// NEP-616: Deploy with deterministically derived account ID.
    DeterministicStateInit(DeterministicStateInitAction),
    /// Transfer NEAR to a gas key.
    TransferToGasKey(TransferToGasKeyAction),
    /// Withdraw NEAR from a gas key.
    WithdrawFromGasKey(WithdrawFromGasKeyAction),
    /// Gas-key-capable delegate action (meta-transactions, NEP-611).
    DelegateV2(Box<VersionedSignedDelegateAction>),
    /// Create a `0u` universal account from its state init.
    UniversalStateInit(UniversalStateInitAction),
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
///
/// Its JSON form is an empty object (`{}`), as in nearcore.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct CreateAccountAction;

impl Serialize for CreateAccountAction {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        serializer.serialize_struct("CreateAccountAction", 0)?.end()
    }
}

impl<'de> Deserialize<'de> for CreateAccountAction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// nearcore's `CreateAccountAction {}`: an object with no fields.
        #[derive(Deserialize)]
        struct Empty {}

        Empty::deserialize(deserializer).map(|Empty {}| CreateAccountAction)
    }
}

/// Deploy contract code.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeployContractAction {
    /// WASM code to deploy.
    #[serde_as(as = "Base64")]
    pub code: Vec<u8>,
}

/// Call a contract function.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct FunctionCallAction {
    /// Method name to call.
    pub method_name: String,
    /// Arguments (JSON or Borsh encoded).
    #[serde_as(as = "Base64")]
    pub args: Vec<u8>,
    /// Gas to attach.
    #[serde(serialize_with = "gas_as_number")]
    pub gas: Gas,
    /// NEAR tokens to attach.
    pub deposit: NearToken,
}

/// Transfer NEAR tokens.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct TransferAction {
    /// Amount to transfer.
    pub deposit: NearToken,
}

/// Stake NEAR for validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct StakeAction {
    /// Amount to stake.
    pub stake: NearToken,
    /// Validator public key.
    pub public_key: PublicKey,
}

/// Add an access key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct AddKeyAction {
    /// Public key to add.
    pub public_key: PublicKey,
    /// Access key details.
    pub access_key: AccessKey,
}

/// Delete an access key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeleteKeyAction {
    /// Public key to delete.
    pub public_key: PublicKey,
}

/// Delete the account.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeleteAccountAction {
    /// Account to receive remaining balance.
    pub beneficiary_id: AccountId,
}

/// Publish a contract to the global registry.
///
/// Global contracts are deployed once and can be referenced by multiple accounts,
/// saving storage costs. The contract can be identified either by its code hash
/// (immutable) or by the publishing account (updatable).
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeployGlobalContractAction {
    /// The WASM code to publish.
    #[serde_as(as = "Base64")]
    pub code: Vec<u8>,
    /// How the contract will be identified.
    pub deploy_mode: PublishMode,
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

// Allowed by the orphan rule because `CryptoHash` is local. Upstream already
// converts from `[u8; 32]` and `AccountId`.
impl From<CryptoHash> for GlobalContractId {
    fn from(code_hash: CryptoHash) -> Self {
        Self::CodeHash(*code_hash.as_bytes())
    }
}

/// Deploy a contract with a deterministically derived account ID (NEP-616).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct DeterministicStateInitAction {
    /// The state initialization data.
    pub state_init: StateInit,
    /// Amount to attach for storage costs.
    pub deposit: NearToken,
}

impl DeterministicStateInitAction {
    /// Derive the deterministic account ID for this action:
    /// `"0s" + hex(keccak256(borsh(state_init))[12..32])`, the same id as
    /// [`StateInit::derive_account_id`] and nearcore's
    /// `derive_near_deterministic_account_id`.
    ///
    /// Hashed in pure Rust. near-global-contracts' `StateInit::derive_account_id`
    /// switches to the `keccak256` host function under its
    /// `__near-sdk-unit-testing` feature, which `near-sdk/unit-testing` anywhere
    /// in a workspace turns on, and a test binary that links near-kit but not
    /// near-sdk then fails to link.
    pub fn derive_account_id(&self) -> AccountId {
        use sha3::{Digest as _, Keccak256};

        let bytes = borsh::to_vec(&self.state_init).expect("borsh into a Vec never fails");
        let hash: [u8; 32] = Keccak256::digest(&bytes).into();
        format!("0s{}", hex::encode(&hash[12..32]))
            .parse()
            .expect("a 0s id is always a valid account id: 42 lowercase hex characters")
    }
}

/// Transfer NEAR to a gas key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct TransferToGasKeyAction {
    /// Public key of the gas key to fund.
    pub public_key: PublicKey,
    /// Amount of NEAR to transfer.
    pub deposit: NearToken,
}

/// Withdraw NEAR from a gas key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct WithdrawFromGasKeyAction {
    /// Public key of the gas key to withdraw from.
    pub public_key: PublicKey,
    /// Amount of NEAR to withdraw.
    pub amount: NearToken,
}

/// Create a `0u` universal account from its state init.
///
/// `state_init` is the borsh encoding of a [`UniversalStateInit`], carried
/// verbatim: the receiver's `0u` account id commits to exactly these bytes,
/// so they must not be re-encoded. Build one with
/// [`Action::universal_state_init`]; decode one with
/// [`decode_state_init`](Self::decode_state_init).
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct UniversalStateInitAction {
    /// Borsh-encoded `UniversalStateInit` bytes.
    #[serde_as(as = "Base64")]
    pub state_init: Vec<u8>,
    /// Amount to attach for the new account's storage staking.
    pub deposit: NearToken,
}

impl UniversalStateInitAction {
    /// The `0u` universal account id this action creates: SHA3-256 over the
    /// carried `state_init` bytes, exactly as given.
    ///
    /// Does not validate the bytes, like nearcore's
    /// `derive_universal_account_id`: a malformed or non-canonical payload
    /// still derives an id, but the chain will never create that account.
    /// To check first, use `RawStateInit::try_derive_account_id` or
    /// [`UniversalStateInit::from_raw`].
    pub fn derive_account_id(&self) -> AccountId {
        super::universal_state_init::derive_universal_account_id(&self.state_init)
    }

    /// Decode the carried bytes leniently; see [`UniversalStateInit::from_raw_lenient`].
    pub fn decode_state_init(&self) -> Result<UniversalStateInit, StateInitError> {
        super::universal_state_init::decode_lenient(&self.state_init)
    }
}

/// Delegate action for meta-transactions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct SignedDelegateAction {
    /// The delegate action.
    pub delegate_action: DelegateAction,
    /// Signature over the delegate action.
    pub signature: super::Signature,
}

/// Delegate action with gas-key support (NEP-611).
///
/// Like the NEP-366 [`DelegateAction`] but its `nonce` is a [`TransactionNonce`]
/// (so it can select one of a gas key's parallel nonces), mirroring
/// [`TransactionV1`](crate::protocol::TransactionV1). Carried inside
/// [`VersionedDelegateActionPayload`] and signed under the V2 domain tag.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum VersionedDelegateActionPayload {
    /// The V2 (gas-key) delegate action.
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
        borsh::to_vec(&(DELEGATE_V2_ACTION_PREFIX, self)).expect("borsh to Vec cannot fail")
    }

    /// The NEP-461 hash signed over for this payload (the V2 signing domain).
    pub fn get_hash(&self) -> CryptoHash {
        CryptoHash::hash(&self.serialize_for_signing())
    }

    /// Attach a signature made over [`get_hash`](Self::get_hash), producing a
    /// [`VersionedSignedDelegateAction`].
    pub fn into_signed(self, signature: Signature) -> VersionedSignedDelegateAction {
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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
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
        Ok(Self::from_bytes(&STANDARD.decode(s)?)?)
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

/// An [`Action`] that is not a delegate (`Delegate` or `DelegateV2`), for use
/// inside a delegate action, which can't nest another.
///
/// It encodes exactly as the inner [`Action`] in borsh and JSON. Decoding
/// rejects delegate variants, as nearcore's `NonDelegateAction` does.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, Serialize)]
#[serde(transparent)]
pub struct NonDelegateAction(Action);

impl<'de> Deserialize<'de> for NonDelegateAction {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_action(<Action as Deserialize>::deserialize(deserializer)?).ok_or_else(|| {
            serde::de::Error::custom("a delegate action must not contain a nested delegate action")
        })
    }
}

impl BorshDeserialize for NonDelegateAction {
    fn deserialize_reader<R: std::io::Read>(reader: &mut R) -> std::io::Result<Self> {
        // Read the Action enum discriminant, reject the delegate variants, then
        // deserialize the rest of the variant in place. Using `EnumExt` (rather
        // than re-prepending the byte to a chained reader) keeps this allocation-
        // free and mirrors nearcore's NonDelegateAction.
        let discriminant = u8::deserialize_reader(reader)?;
        if DELEGATE_VARIANT_DISCRIMINANTS.contains(&discriminant) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "a delegate action must not contain a nested delegate action",
            ));
        }
        Action::deserialize_variant(reader, discriminant).map(Self)
    }
}

impl Action {
    /// Create a CreateAccount action.
    pub fn create_account() -> Self {
        Self::CreateAccount(CreateAccountAction)
    }

    /// Create a DeployContract action.
    pub fn deploy_contract(code: impl Into<Vec<u8>>) -> Self {
        Self::DeployContract(DeployContractAction { code: code.into() })
    }

    /// Create a FunctionCall action.
    pub fn function_call(
        method_name: impl Into<String>,
        args: impl Into<Vec<u8>>,
        gas: Gas,
        deposit: NearToken,
    ) -> Self {
        Self::FunctionCall(FunctionCallAction {
            method_name: method_name.into(),
            args: args.into(),
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
            access_key: AccessKey {
                nonce: 0,
                permission: AccessKeyPermission::FunctionCall(FunctionCallPermission {
                    allowance,
                    receiver_id,
                    method_names,
                }),
            },
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

    /// Publish a contract to the global registry (see [`DeployGlobalContractAction`]).
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::protocol::{Action, PublishMode};
    ///
    /// # let wasm_code = vec![0u8; 8];
    /// // Publish updatable contract (identified by your account)
    /// let updatable = Action::publish(wasm_code.clone(), PublishMode::Updatable);
    ///
    /// // Publish immutable contract (identified by its hash)
    /// let immutable = Action::publish(wasm_code, PublishMode::Immutable);
    /// ```
    pub fn publish(code: impl Into<Vec<u8>>, mode: PublishMode) -> Self {
        Self::DeployGlobalContract(DeployGlobalContractAction {
            code: code.into(),
            deploy_mode: mode,
        })
    }

    /// Deploy a contract from the global registry.
    ///
    /// Pass a [`CryptoHash`] to use a contract published with
    /// [`PublishMode::Immutable`] (pinned to that code), or an [`AccountId`]
    /// to use the contract that account published with
    /// [`PublishMode::Updatable`] (it follows the publisher's updates).
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::protocol::Action;
    /// use near_kit_primitives::{CryptoHash, account};
    ///
    /// let by_publisher = Action::deploy_from(account!("publisher.near"));
    /// let by_hash = Action::deploy_from(CryptoHash::hash(b"wasm"));
    /// ```
    pub fn deploy_from(code: impl Into<GlobalContractId>) -> Self {
        Self::UseGlobalContract(UseGlobalContractAction {
            contract_identifier: code.into(),
        })
    }

    /// Create a NEP-616 deterministic state init action.
    ///
    /// The account ID is derived from the state init data:
    /// `"0s" + hex(keccak256(borsh(state_init))[12..32])`. Send the
    /// transaction to that account ([`DeterministicStateInitAction::derive_account_id`]);
    /// near-kit's `Tx` rejects any other receiver before signing.
    pub fn state_init(state_init: StateInit, deposit: NearToken) -> Self {
        Self::DeterministicStateInit(DeterministicStateInitAction {
            state_init,
            deposit,
        })
    }

    /// Create a `0u` universal account from its state init (nearcore 2.14).
    ///
    /// Accepts a typed [`UniversalStateInit`] /
    /// [`UniversalStateInitV1`](super::UniversalStateInitV1) (encoded
    /// canonically), the same from near-global-contracts or near-sdk, or a
    /// [`RawStateInit`] / `Vec<u8>` carried verbatim. The transaction's
    /// receiver must be the derived account id; see
    /// [`UniversalStateInitAction::derive_account_id`].
    ///
    /// # Example
    ///
    /// ```rust
    /// # use near_kit_primitives::*;
    /// # use near_kit_primitives::protocol::{Action, UniversalStateInitV1};
    /// # use near_kit_primitives::signer::PublicKey;
    /// let key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
    /// let action = Action::universal_state_init(
    ///     UniversalStateInitV1::new().with_access_key(key),
    ///     NearToken::from_millinear(1),
    /// );
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn universal_state_init(state_init: impl Into<RawStateInit>, deposit: NearToken) -> Self {
        Self::UniversalStateInit(UniversalStateInitAction {
            state_init: state_init.into().0,
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
    /// Per NEP-461 this is [`DELEGATE_ACTION_PREFIX`] (u32 LE) then borsh of the
    /// action, so a signed delegate action is never a signed transaction.
    ///
    /// # Example
    ///
    /// ```rust
    /// use near_kit_primitives::CryptoHash;
    /// use near_kit_primitives::protocol::{DelegateAction, SignedDelegateAction};
    /// use near_kit_primitives::signer::SecretKey;
    ///
    /// // What `get_hash` and a signer do for you.
    /// fn sign(delegate_action: DelegateAction, key: &SecretKey) -> SignedDelegateAction {
    ///     let bytes = delegate_action.serialize_for_signing();
    ///     let hash = CryptoHash::hash(&bytes);
    ///     let signature = key.sign(hash.as_bytes());
    ///     delegate_action.into_signed(signature)
    /// }
    /// ```
    pub fn serialize_for_signing(&self) -> Vec<u8> {
        borsh::to_vec(&(DELEGATE_ACTION_PREFIX, self)).expect("borsh to Vec cannot fail")
    }

    /// Get the hash of this delegate action (for signing).
    pub fn get_hash(&self) -> CryptoHash {
        CryptoHash::hash(&self.serialize_for_signing())
    }

    /// Attach a signature made over [`get_hash`](Self::get_hash), producing a
    /// [`SignedDelegateAction`].
    pub fn into_signed(self, signature: Signature) -> SignedDelegateAction {
        SignedDelegateAction {
            delegate_action: self,
            signature,
        }
    }
}

impl SignedDelegateAction {
    /// Verify the signature against the delegate action's public key under the
    /// NEP-366 (V1) domain.
    pub fn verify(&self) -> bool {
        let hash = self.delegate_action.get_hash();
        self.signature
            .verify(hash.as_bytes(), &self.delegate_action.public_key)
    }

    /// Encode the signed delegate action to bytes for transport.
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("signed delegate action serialization should never fail")
    }

    /// Encode the signed delegate action to base64 for transport.
    pub fn to_base64(&self) -> String {
        STANDARD.encode(self.to_bytes())
    }

    /// Decode a signed delegate action from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, borsh::io::Error> {
        borsh::from_slice(bytes)
    }

    /// Decode a signed delegate action from base64.
    pub fn from_base64(s: &str) -> Result<Self, DecodeError> {
        Ok(Self::from_bytes(&STANDARD.decode(s)?)?)
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

/// nearcore writes an action's gas as a JSON number (its `Gas` keeps the
/// number form for RPC compatibility); `NearGas` alone would write a string.
/// Reading accepts both, like `NearGas`.
fn gas_as_number<S: serde::Serializer>(gas: &Gas, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(gas.as_gas())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Gas, NearToken, SecretKey};
    use std::collections::BTreeMap;

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
    fn test_signed_delegate_action_verify() {
        let secret_key = SecretKey::generate_ed25519();
        let mut delegate_action = create_test_delegate_action();
        delegate_action.public_key = secret_key.public_key();
        let signature = secret_key.sign(delegate_action.get_hash().as_bytes());
        let signed = delegate_action.into_signed(signature);
        assert!(signed.verify());

        // Any change to the signed fields invalidates the signature.
        let mut tampered = signed.clone();
        tampered.delegate_action.nonce += 1;
        assert!(!tampered.verify());

        let mut tampered = signed.clone();
        tampered.delegate_action.receiver_id = "mallory.testnet".parse().unwrap();
        assert!(!tampered.verify());

        // A signature from another key fails against the stated public key.
        let mut wrong_key = signed;
        wrong_key.signature =
            SecretKey::generate_ed25519().sign(wrong_key.delegate_action.get_hash().as_bytes());
        assert!(!wrong_key.verify());
    }

    #[test]
    fn test_signed_delegate_action_roundtrips() {
        let delegate_action = create_test_delegate_action();
        let signature = SecretKey::generate_ed25519().sign(delegate_action.get_hash().as_bytes());
        let signed = delegate_action.into_signed(signature);

        assert_eq!(
            SignedDelegateAction::from_bytes(&signed.to_bytes()).unwrap(),
            signed
        );
        assert_eq!(
            SignedDelegateAction::from_base64(&signed.to_base64()).unwrap(),
            signed
        );
        assert_eq!(signed.sender_id().as_str(), "alice.testnet");
        assert_eq!(signed.receiver_id().as_str(), "bob.testnet");
    }

    /// Every variant's borsh tag is its nearcore discriminant, and it round-trips.
    #[test]
    fn test_action_discriminants() {
        let code_hash = CryptoHash::hash(&[1, 2, 3]);
        let v1 = create_test_delegate_action().into_signed(sig());
        let v2 = VersionedDelegateActionPayload::from(sample_delegate_v2(pk())).into_signed(sig());
        let actions = [
            Action::create_account(),
            Action::deploy_contract(vec![1, 2, 3]),
            Action::function_call("m", vec![], Gas::from_gas(1), NearToken::ZERO),
            Action::transfer(NearToken::from_near(1)),
            Action::stake(NearToken::from_near(1), pk()),
            Action::add_full_access_key(pk()),
            Action::delete_key(pk()),
            Action::delete_account("b.near".parse().unwrap()),
            Action::delegate(v1),
            Action::publish(vec![1, 2, 3], PublishMode::Updatable),
            Action::deploy_from(code_hash),
            Action::state_init(StateInitV1::code(code_hash).into(), NearToken::from_near(1)),
            Action::transfer_to_gas_key(pk(), NearToken::from_near(1)),
            Action::withdraw_from_gas_key(pk(), NearToken::from_near(1)),
            Action::delegate_v2(v2),
            Action::universal_state_init(vec![1, 2, 3], NearToken::from_near(1)),
        ];
        for (tag, action) in actions.into_iter().enumerate() {
            let bytes = borsh::to_vec(&action).unwrap();
            assert_eq!(usize::from(bytes[0]), tag, "{action:?}");
            assert_eq!(borsh::from_slice::<Action>(&bytes).unwrap(), action);
        }
    }

    #[test]
    fn test_universal_state_init_wire_format() {
        let action = Action::UniversalStateInit(UniversalStateInitAction {
            state_init: vec![0xaa, 0xbb, 0xcc],
            deposit: NearToken::from_yoctonear(7),
        });
        let bytes = borsh::to_vec(&action).unwrap();
        // discriminant 15, then state_init as a u32-length-prefixed byte
        // vector (nearcore's `RawStateInit`), then the u128 deposit.
        let mut expected = vec![15, 3, 0, 0, 0, 0xaa, 0xbb, 0xcc];
        expected.extend_from_slice(&7u128.to_le_bytes());
        assert_eq!(bytes, expected);
        assert_eq!(borsh::from_slice::<Action>(&bytes).unwrap(), action);
    }

    #[test]
    fn test_universal_state_init_constructor() {
        use crate::types::{PublicKeyHandle, UniversalStateInitV1};

        // nearcore 2.14-release `test_derive_universal_account_id` key-only vector.
        let typed: UniversalStateInit = UniversalStateInitV1::new()
            .with_access_key(PublicKeyHandle::MlDsa65Hash([0x11; 32]))
            .into();
        let raw = typed.to_raw();
        let deposit = NearToken::from_yoctonear(7);

        let Action::UniversalStateInit(action) = Action::universal_state_init(&typed, deposit)
        else {
            panic!("expected a UniversalStateInit action");
        };
        assert_eq!(action.state_init, raw.0);
        assert_eq!(
            action.derive_account_id().as_str(),
            "0ux8te7g99f9kqzdtp9h4qnwt9aczpgayymmtbdc50w199rcw3at1g"
        );
        assert_eq!(action.decode_state_init().unwrap(), typed);

        // Raw bytes are carried verbatim, even non-canonical ones, and the id
        // is over them as given.
        let mut unsorted = vec![0, 0, 2, 0, 0, 0];
        for key in *b"ba" {
            unsorted.extend_from_slice(&[1, 0, 0, 0, key, 0, 0, 0, 0]);
        }
        unsorted.extend_from_slice(&[0, 0, 0, 0]);
        let Action::UniversalStateInit(action) =
            Action::universal_state_init(unsorted.clone(), deposit)
        else {
            panic!("expected a UniversalStateInit action");
        };
        assert_eq!(action.state_init, unsorted);
        assert_eq!(
            action.derive_account_id(),
            crate::types::universal_state_init::derive_universal_account_id(&unsorted)
        );
    }

    #[test]
    fn test_publish_mode_wire_compatibility() {
        assert_eq!(borsh::to_vec(&PublishMode::Immutable).unwrap(), [0]);
        assert_eq!(borsh::to_vec(&PublishMode::Updatable).unwrap(), [1]);
    }

    #[test]
    fn test_access_key_permission_discriminants() {
        let fc = FunctionCallPermission {
            allowance: None,
            receiver_id: "test.near".parse().unwrap(),
            method_names: vec![],
        };
        let gas_key = GasKeyInfo {
            balance: NearToken::from_near(1),
            num_nonces: 5,
        };
        let permissions = [
            AccessKeyPermission::FunctionCall(fc.clone()),
            AccessKeyPermission::FullAccess,
            AccessKeyPermission::GasKeyFunctionCall(gas_key.clone(), fc),
            AccessKeyPermission::GasKeyFullAccess(gas_key),
        ];
        for (tag, permission) in permissions.iter().enumerate() {
            assert_eq!(
                usize::from(borsh::to_vec(permission).unwrap()[0]),
                tag,
                "{permission:?}"
            );
        }
    }

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
        let signed = payload.into_signed(sig);
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
            .into_signed(secret.sign(payload.get_hash().as_bytes()));

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

    /// Both delegate variants are delegates, and `NonDelegateAction` rejects
    /// them through the typed constructor and on the wire (borsh), as nearcore
    /// does.
    #[test]
    fn test_non_delegate_rejects_all_delegate_variants() {
        let secret = SecretKey::generate_ed25519();

        let v1_signed = create_test_delegate_action()
            .into_signed(secret.sign(create_test_delegate_action().get_hash().as_bytes()));
        let v1 = Action::delegate(v1_signed);

        let payload: VersionedDelegateActionPayload =
            sample_delegate_v2(secret.public_key()).into();
        let v2 = Action::delegate_v2(
            payload
                .clone()
                .into_signed(secret.sign(payload.get_hash().as_bytes())),
        );

        for (action, tag) in [v1, v2].into_iter().zip(DELEGATE_VARIANT_DISCRIMINANTS) {
            assert!(action.is_delegate());
            // Typed path rejects.
            assert!(NonDelegateAction::from_action(action.clone()).is_none());
            assert!(NonDelegateAction::try_from(action.clone()).is_err());

            // Borsh path rejects (the bytes of a delegate action embedded where a
            // NonDelegateAction is expected).
            let bytes = borsh::to_vec(&action).unwrap();
            assert_eq!(bytes[0], tag);
            let err = NonDelegateAction::try_from_slice(&bytes).unwrap_err();
            assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        }

        // A non-delegate action is accepted on both paths.
        let transfer = Action::transfer(NearToken::from_near(1));
        assert!(!transfer.is_delegate());
        assert!(NonDelegateAction::from_action(transfer.clone()).is_some());
        let bytes = borsh::to_vec(&transfer).unwrap();
        assert!(NonDelegateAction::try_from_slice(&bytes).is_ok());
    }

    // JSON: nearcore's protocol `Action` shape (core/primitives/src/action)

    const PK: &str = "ed25519:A435yoEuewCD9sg3rjdsUSmkcJkUaHvjEV5CHddog2NK";

    fn pk() -> PublicKey {
        PK.parse().unwrap()
    }

    fn sig() -> Signature {
        Signature::Ed25519([7; 64])
    }

    fn assert_json(action: Action, json: serde_json::Value) {
        assert_eq!(serde_json::to_value(&action).unwrap(), json, "{action:?}");
        assert_eq!(
            serde_json::from_value::<Action>(json.clone()).unwrap(),
            action,
            "{json}"
        );
    }

    #[test]
    fn action_json_matches_nearcore() {
        use serde_json::json;
        let hash = CryptoHash::hash(b"code");
        let fc_permission = FunctionCallPermission {
            allowance: Some(NearToken::from_yoctonear(5)),
            receiver_id: "c.near".parse().unwrap(),
            method_names: vec!["m".to_string()],
        };
        let gas_key = GasKeyInfo {
            balance: NearToken::from_yoctonear(9),
            num_nonces: 4,
        };
        let state_init = StateInit::V1(StateInitV1 {
            code: GlobalContractId::from(hash),
            data: BTreeMap::from([(b"k".to_vec(), b"v".to_vec())]),
        });
        let universal = vec![1, 2, 3];

        let cases = [
            (Action::create_account(), json!({"CreateAccount": {}})),
            (
                Action::deploy_contract(b"\0asm".to_vec()),
                json!({"DeployContract": {"code": "AGFzbQ=="}}),
            ),
            (
                Action::FunctionCall(FunctionCallAction {
                    method_name: "m".to_string(),
                    args: b"{}".to_vec(),
                    gas: Gas::from_tgas(30),
                    deposit: NearToken::from_yoctonear(1),
                }),
                json!({"FunctionCall": {
                    "method_name": "m", "args": "e30=", "gas": 30_000_000_000_000u64, "deposit": "1"
                }}),
            ),
            (
                Action::transfer(NearToken::from_yoctonear(2)),
                json!({"Transfer": {"deposit": "2"}}),
            ),
            (
                Action::Stake(StakeAction {
                    stake: NearToken::from_yoctonear(3),
                    public_key: pk(),
                }),
                json!({"Stake": {"stake": "3", "public_key": PK}}),
            ),
            (
                Action::add_full_access_key(pk()),
                json!({"AddKey": {"public_key": PK, "access_key": {"nonce": 0, "permission": "FullAccess"}}}),
            ),
            (
                Action::AddKey(AddKeyAction {
                    public_key: pk(),
                    access_key: AccessKey {
                        nonce: 0,
                        permission: AccessKeyPermission::FunctionCall(fc_permission.clone()),
                    },
                }),
                json!({"AddKey": {"public_key": PK, "access_key": {"nonce": 0, "permission": {
                    "FunctionCall": {"allowance": "5", "receiver_id": "c.near", "method_names": ["m"]}
                }}}}),
            ),
            (
                Action::AddKey(AddKeyAction {
                    public_key: pk(),
                    access_key: AccessKey {
                        nonce: 0,
                        permission: AccessKeyPermission::GasKeyFunctionCall(
                            gas_key.clone(),
                            fc_permission.clone(),
                        ),
                    },
                }),
                json!({"AddKey": {"public_key": PK, "access_key": {"nonce": 0, "permission": {
                    "GasKeyFunctionCall": [
                        {"balance": "9", "num_nonces": 4},
                        {"allowance": "5", "receiver_id": "c.near", "method_names": ["m"]}
                    ]
                }}}}),
            ),
            (
                Action::AddKey(AddKeyAction {
                    public_key: pk(),
                    access_key: AccessKey {
                        nonce: 0,
                        permission: AccessKeyPermission::GasKeyFullAccess(gas_key),
                    },
                }),
                json!({"AddKey": {"public_key": PK, "access_key": {"nonce": 0, "permission": {
                    "GasKeyFullAccess": {"balance": "9", "num_nonces": 4}
                }}}}),
            ),
            (
                Action::delete_key(pk()),
                json!({"DeleteKey": {"public_key": PK}}),
            ),
            (
                Action::delete_account("b.near".parse().unwrap()),
                json!({"DeleteAccount": {"beneficiary_id": "b.near"}}),
            ),
            (
                Action::publish(b"\0asm".to_vec(), PublishMode::Updatable),
                json!({"DeployGlobalContract": {"code": "AGFzbQ==", "deploy_mode": "AccountId"}}),
            ),
            (
                Action::publish(b"\0asm".to_vec(), PublishMode::Immutable),
                json!({"DeployGlobalContract": {"code": "AGFzbQ==", "deploy_mode": "CodeHash"}}),
            ),
            (
                Action::deploy_from(hash),
                json!({"UseGlobalContract": {"contract_identifier": {"hash": hash.to_string()}}}),
            ),
            (
                Action::deploy_from("p.near".parse::<AccountId>().unwrap()),
                json!({"UseGlobalContract": {"contract_identifier": {"account_id": "p.near"}}}),
            ),
            (
                Action::DeterministicStateInit(DeterministicStateInitAction {
                    state_init,
                    deposit: NearToken::from_yoctonear(4),
                }),
                json!({"DeterministicStateInit": {
                    "state_init": {"V1": {"code": {"hash": hash.to_string()}, "data": {"aw==": "dg=="}}},
                    "deposit": "4"
                }}),
            ),
            (
                Action::transfer_to_gas_key(pk(), NearToken::from_yoctonear(6)),
                json!({"TransferToGasKey": {"public_key": PK, "deposit": "6"}}),
            ),
            (
                Action::withdraw_from_gas_key(pk(), NearToken::from_yoctonear(7)),
                json!({"WithdrawFromGasKey": {"public_key": PK, "amount": "7"}}),
            ),
            (
                Action::UniversalStateInit(UniversalStateInitAction {
                    state_init: universal,
                    deposit: NearToken::from_yoctonear(8),
                }),
                json!({"UniversalStateInit": {"state_init": "AQID", "deposit": "8"}}),
            ),
        ];
        for (action, json) in cases {
            assert_json(action, json);
        }
    }

    #[test]
    fn delegate_action_json_matches_nearcore() {
        use serde_json::json;
        let inner = vec![NonDelegateAction::from_action(Action::create_account()).unwrap()];
        let v1 = SignedDelegateAction {
            delegate_action: DelegateAction {
                sender_id: "a.near".parse().unwrap(),
                receiver_id: "b.near".parse().unwrap(),
                actions: inner.clone(),
                nonce: 1,
                max_block_height: 2,
                public_key: pk(),
            },
            signature: sig(),
        };
        assert_json(
            Action::Delegate(Box::new(v1)),
            json!({"Delegate": {
                "delegate_action": {
                    "sender_id": "a.near", "receiver_id": "b.near",
                    "actions": [{"CreateAccount": {}}],
                    "nonce": 1, "max_block_height": 2, "public_key": PK
                },
                "signature": sig().to_string()
            }}),
        );

        let v2 = VersionedSignedDelegateAction {
            delegate_action: VersionedDelegateActionPayload::V2(DelegateActionV2 {
                sender_id: "a.near".parse().unwrap(),
                receiver_id: "b.near".parse().unwrap(),
                actions: inner,
                nonce: TransactionNonce::from_nonce_and_index(3, 1),
                max_block_height: 2,
                public_key: pk(),
            }),
            signature: sig(),
        };
        assert_json(
            Action::DelegateV2(Box::new(v2)),
            json!({"DelegateV2": {
                "delegate_action": {"V2": {
                    "sender_id": "a.near", "receiver_id": "b.near",
                    "actions": [{"CreateAccount": {}}],
                    "nonce": {"GasKeyNonce": {"nonce": 3, "nonce_index": 1}},
                    "max_block_height": 2, "public_key": PK
                }},
                "signature": sig().to_string()
            }}),
        );
    }

    #[test]
    fn action_json_accepts_gas_as_string() {
        let action: Action = serde_json::from_value(serde_json::json!({"FunctionCall": {
            "method_name": "m", "args": "", "gas": "5", "deposit": "0"
        }}))
        .unwrap();
        let Action::FunctionCall(call) = action else {
            panic!("expected FunctionCall");
        };
        assert_eq!(call.gas, Gas::from_gas(5));
    }

    #[test]
    fn non_delegate_action_json_rejects_nested_delegates() {
        let nested = serde_json::json!({"Delegate": {
            "delegate_action": {
                "sender_id": "a.near", "receiver_id": "b.near", "actions": [],
                "nonce": 1, "max_block_height": 2, "public_key": PK
            },
            "signature": sig().to_string()
        }});
        assert!(serde_json::from_value::<Action>(nested.clone()).is_ok());
        let err = serde_json::from_value::<NonDelegateAction>(nested).unwrap_err();
        assert!(err.to_string().contains("nested delegate"), "{err}");
        assert_eq!(
            serde_json::from_value::<NonDelegateAction>(serde_json::json!({"CreateAccount": {}}))
                .unwrap()
                .into_inner(),
            Action::create_account()
        );
    }
}
