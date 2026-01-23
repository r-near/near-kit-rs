//! Transaction action types.

use base64::{engine::general_purpose::STANDARD, Engine as _};
use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use super::{AccountId, CryptoHash, Gas, NearToken, PublicKey, Signature};

/// NEP-461 prefix for delegate actions (meta-transactions).
/// Value: 2^30 + 366 = 1073742190
///
/// This prefix is prepended to DelegateAction when serializing for signing,
/// ensuring delegate action signatures are always distinguishable from
/// regular transaction signatures.
pub const DELEGATE_ACTION_PREFIX: u32 = 1_073_742_190;

/// Access key permission.
///
/// IMPORTANT: Variant order matters for Borsh serialization!
/// NEAR Protocol defines: 0 = FunctionCall, 1 = FullAccess
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum AccessKeyPermission {
    /// Function call access with restrictions. (discriminant = 0)
    FunctionCall(FunctionCallPermission),
    /// Full access to the account. (discriminant = 1)
    FullAccess,
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
/// 9 = DeployGlobalContract, 10 = UseGlobalContract, 11 = DeterministicStateInit
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

/// How a global contract is identified in the registry.
///
/// Global contracts can be referenced either by their code hash (immutable)
/// or by the account that published them (updatable).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum GlobalContractIdentifier {
    /// Reference by code hash (32-byte SHA-256 hash of the WASM code).
    /// This creates an immutable reference - the contract cannot be updated.
    CodeHash(CryptoHash),
    /// Reference by the account ID that published the contract.
    /// The publisher can update the contract, and all users will get the new version.
    AccountId(AccountId),
}

/// Deploy mode for global contracts.
///
/// Determines how the contract will be identified in the global registry.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum GlobalContractDeployMode {
    /// Contract is identified by its code hash (immutable).
    /// Other accounts reference it by the hash.
    CodeHash,
    /// Contract is identified by the signer's account ID (updatable).
    /// The signer can update the contract later.
    AccountId,
}

/// Deploy a contract to the global registry.
///
/// Global contracts are deployed once and can be referenced by multiple accounts,
/// saving storage costs. The contract can be identified either by its code hash
/// (immutable) or by the publishing account (updatable).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct UseGlobalContractAction {
    /// Reference to the published contract.
    pub contract_identifier: GlobalContractIdentifier,
}

// ============================================================================
// NEP-616 Deterministic Account Actions
// ============================================================================

/// State initialization data for NEP-616 deterministic accounts.
///
/// The account ID is derived from: `"0s" + hex(keccak256(borsh(state_init))[12..32])`
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum DeterministicAccountStateInit {
    /// Version 1 of the state init format.
    V1(DeterministicAccountStateInitV1),
}

/// Version 1 of deterministic account state initialization.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DeterministicAccountStateInitV1 {
    /// Reference to the contract code (from global registry).
    pub code: GlobalContractIdentifier,
    /// Initial key-value pairs to populate in the contract's storage.
    /// Keys and values are Borsh-serialized bytes.
    pub data: Vec<(Vec<u8>, Vec<u8>)>,
}

/// Deploy a contract with a deterministically derived account ID (NEP-616).
///
/// This enables creating accounts where the account ID is derived from the
/// contract code and initial state, making them predictable and reproducible.
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct DeterministicStateInitAction {
    /// The state initialization data.
    pub state_init: DeterministicAccountStateInit,
    /// Amount to attach for storage costs.
    pub deposit: NearToken,
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

/// Non-delegate action (for use within DelegateAction).
///
/// This is the same as Action but without the Delegate variant,
/// since delegate actions cannot contain nested delegate actions.
///
/// IMPORTANT: Variant order matters for Borsh serialization!
/// The discriminants must match the Action enum (excluding Delegate).
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum NonDelegateAction {
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
    /// Publish a contract to global registry. (discriminant = 8)
    /// Note: This follows after DeleteAccount since Delegate is not included.
    DeployGlobalContract(DeployGlobalContractAction),
    /// Deploy from a previously published global contract. (discriminant = 9)
    UseGlobalContract(UseGlobalContractAction),
    /// NEP-616: Deploy with deterministically derived account ID. (discriminant = 10)
    DeterministicStateInit(DeterministicStateInitAction),
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

    /// Publish a contract to the global registry.
    ///
    /// Global contracts are deployed once and can be referenced by multiple accounts,
    /// saving storage costs.
    ///
    /// # Arguments
    ///
    /// * `code` - The WASM code to publish
    /// * `by_hash` - If true, contract is identified by its code hash (immutable).
    ///   If false (default), contract is identified by the signer's account ID (updatable).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Publish updatable contract (identified by your account)
    /// near.transaction("alice.near")
    ///     .publish_contract(wasm_code, false)
    ///     .send()
    ///     .await?;
    ///
    /// // Publish immutable contract (identified by its hash)
    /// near.transaction("alice.near")
    ///     .publish_contract(wasm_code, true)
    ///     .send()
    ///     .await?;
    /// ```
    pub fn publish_contract(code: Vec<u8>, by_hash: bool) -> Self {
        Self::DeployGlobalContract(DeployGlobalContractAction {
            code,
            deploy_mode: if by_hash {
                GlobalContractDeployMode::CodeHash
            } else {
                GlobalContractDeployMode::AccountId
            },
        })
    }

    /// Deploy a contract from the global registry by code hash.
    ///
    /// References a previously published immutable contract.
    pub fn deploy_from_hash(code_hash: CryptoHash) -> Self {
        Self::UseGlobalContract(UseGlobalContractAction {
            contract_identifier: GlobalContractIdentifier::CodeHash(code_hash),
        })
    }

    /// Deploy a contract from the global registry by account ID.
    ///
    /// References a contract published by the given account.
    /// The contract can be updated by the publisher.
    pub fn deploy_from_account(account_id: AccountId) -> Self {
        Self::UseGlobalContract(UseGlobalContractAction {
            contract_identifier: GlobalContractIdentifier::AccountId(account_id),
        })
    }

    /// Create a NEP-616 deterministic state init action.
    ///
    /// The account ID is derived from the state init data:
    /// `"0s" + hex(keccak256(borsh(state_init))[12..32])`
    pub fn state_init(state_init: DeterministicAccountStateInit, deposit: NearToken) -> Self {
        Self::DeterministicStateInit(DeterministicStateInitAction {
            state_init,
            deposit,
        })
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
#[derive(Debug)]
pub enum DecodeError {
    /// Base64 decoding failed.
    Base64(base64::DecodeError),
    /// Borsh deserialization failed.
    Borsh(borsh::io::Error),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Base64(e) => write!(f, "base64 decode error: {}", e),
            DecodeError::Borsh(e) => write!(f, "borsh decode error: {}", e),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DecodeError::Base64(e) => Some(e),
            DecodeError::Borsh(e) => Some(e),
        }
    }
}

impl NonDelegateAction {
    /// Convert from an Action, returning None if it's a Delegate action.
    pub fn from_action(action: Action) -> Option<Self> {
        match action {
            Action::CreateAccount(a) => Some(NonDelegateAction::CreateAccount(a)),
            Action::DeployContract(a) => Some(NonDelegateAction::DeployContract(a)),
            Action::FunctionCall(a) => Some(NonDelegateAction::FunctionCall(a)),
            Action::Transfer(a) => Some(NonDelegateAction::Transfer(a)),
            Action::Stake(a) => Some(NonDelegateAction::Stake(a)),
            Action::AddKey(a) => Some(NonDelegateAction::AddKey(a)),
            Action::DeleteKey(a) => Some(NonDelegateAction::DeleteKey(a)),
            Action::DeleteAccount(a) => Some(NonDelegateAction::DeleteAccount(a)),
            Action::Delegate(_) => None,
            Action::DeployGlobalContract(a) => Some(NonDelegateAction::DeployGlobalContract(a)),
            Action::UseGlobalContract(a) => Some(NonDelegateAction::UseGlobalContract(a)),
            Action::DeterministicStateInit(a) => Some(NonDelegateAction::DeterministicStateInit(a)),
        }
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
            actions: vec![NonDelegateAction::Transfer(TransferAction {
                deposit: NearToken::from_near(1),
            })],
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
}
