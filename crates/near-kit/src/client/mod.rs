//! Client module for NEAR Protocol.

mod near;
mod nonce_manager;
mod query;
mod rpc;
mod signer;
mod transaction;
mod tx;

#[cfg(feature = "keyring")]
mod keyring_signer;

pub use near::{Near, NearBuilder, SandboxNetwork, SANDBOX_ROOT_ACCOUNT, SANDBOX_ROOT_PRIVATE_KEY};
pub use query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
pub use rpc::{RetryConfig, RpcClient};
pub use signer::{
    EnvSigner, FileSigner, InMemorySigner, Nep413SignFuture, RotatingSigner, SignFuture, Signer,
};
pub use transaction::{
    CallBuilder, DelegateOptions, DelegateResult, TransactionBuilder, TransactionSend,
};
pub use tx::{AddKeyCall, ContractCall, DeleteKeyCall, DeployCall, TransferCall};

#[cfg(feature = "keyring")]
pub use keyring_signer::KeyringSigner;
