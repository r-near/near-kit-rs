//! Transaction action types.

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

use super::{AccountId, Gas, NearToken, PublicKey};

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
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum NonDelegateAction {
    CreateAccount(CreateAccountAction),
    DeployContract(DeployContractAction),
    FunctionCall(FunctionCallAction),
    Transfer(TransferAction),
    Stake(StakeAction),
    AddKey(AddKeyAction),
    DeleteKey(DeleteKeyAction),
    DeleteAccount(DeleteAccountAction),
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
}
