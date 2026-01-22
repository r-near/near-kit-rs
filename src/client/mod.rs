//! Client module for NEAR Protocol.

mod keystore;
mod near;
mod query;
mod rpc;
mod signer;
mod tx;

pub use keystore::{InMemoryKeyStore, KeyStore};
pub use near::{Near, NearBuilder};
pub use query::{AccessKeysQuery, AccountExistsQuery, AccountQuery, BalanceQuery, ViewCall};
pub use rpc::{RetryConfig, RpcClient};
pub use signer::{KeyStoreSigner, Signer};
pub use tx::{AddKeyCall, ContractCall, DeleteKeyCall, DeployCall, TransferCall};
