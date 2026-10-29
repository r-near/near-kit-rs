//! The clients, builders, transports and signer loaders.

// Everything that talks to the network lives behind the `rpc` feature; the
// signers stay available in offline builds (they only do local cryptography).
#[cfg(feature = "rpc")]
pub(crate) mod near;
#[cfg(feature = "rpc")]
mod nonce_manager;
#[cfg(feature = "rpc")]
mod query;
#[cfg(feature = "rpc")]
mod rpc;
mod signer;
#[cfg(feature = "rpc")]
mod signing;
#[cfg(all(test, feature = "rpc"))]
mod test_rpc;
#[cfg(feature = "rpc")]
mod transaction;
#[cfg(feature = "rpc")]
mod transport;

#[cfg(feature = "keyring")]
mod keyring;

#[cfg(feature = "rpc")]
pub use near::{Near, NearBuilder};
#[cfg(feature = "rpc")]
pub use query::{AccountExistsQuery, BalanceQuery, TransactionStatusQuery};
#[cfg(feature = "rpc")]
pub use rpc::{RetryConfig, RpcClient};
#[cfg(feature = "file-signer")]
pub use signer::{from_credentials, from_credentials_file};
pub use signer::{from_env, from_env_vars};
#[cfg(feature = "rpc")]
pub use signing::SigningNear;
#[cfg(feature = "rpc")]
pub use transaction::{DelegateOptions, SignedTransactionSend, Tx};
#[cfg(feature = "rpc")]
pub use transport::{BoxFuture, RpcTransport, TransportResponse};
// Only the operational built-in transport matching the build configuration is
// public: reqwest everywhere except WASI, and the wasi:http transport on
// wasm32-wasip2 with `wasi-http`. WASI without it uses a private erroring
// fallback until the caller injects a custom transport.
#[cfg(all(feature = "rpc", not(all(target_arch = "wasm32", target_os = "wasi"))))]
pub use transport::ReqwestTransport;
#[cfg(all(
    feature = "wasi-http",
    target_arch = "wasm32",
    target_os = "wasi",
    target_env = "p2"
))]
pub use transport::WasiHttpTransport;

#[cfg(feature = "keyring")]
pub use keyring::from_keyring;
