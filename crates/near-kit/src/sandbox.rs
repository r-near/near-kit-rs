//! Sandbox testing utilities for NEAR Protocol.
//!
//! This module provides ergonomic APIs for testing against a local NEAR sandbox.
//! It handles sandbox lifecycle management, including shared singleton instances
//! for test performance and fresh instances for isolated tests.
//!
//! The sandbox runs as a Docker container via testcontainers, using the
//! `nearprotocol/sandbox` image from Docker Hub.
//!
//! # Design Principles
//!
//! 1. **Consistent with production**: Test code looks like production code.
//!    `Near` is always the entry point - you just pass a different network.
//!
//! 2. **Sandbox lifecycle management**: The library handles starting/stopping
//!    sandboxes, including a shared singleton for test performance.
//!
//! 3. **No magic account creation**: All accounts are created through the
//!    standard `near.transaction().create_account()` flow.
//!
//! # Example
//!
//! ```rust,ignore
//! use near_kit::*;
//! use near_kit::sandbox::SandboxConfig;
//!
//! #[tokio::test]
//! async fn test_transfer() {
//!     // Same pattern as production - just different network
//!     let near = Near::sandbox(SandboxConfig::shared().await);
//!
//!     // Create accounts the normal way
//!     let alice_key = SecretKey::generate_ed25519();
//!     near.transaction("alice.sandbox")
//!         .create_account()
//!         .transfer("100 NEAR")
//!         .add_full_access_key(alice_key.public_key())
//!         .send()
//!         .await
//!         .unwrap();
//! }
//!
//! #[tokio::test]
//! async fn test_isolated() {
//!     // Fresh sandbox - completely isolated, stopped when container drops
//!     let sandbox = SandboxConfig::fresh().await;
//!     let near = Near::sandbox(&sandbox);
//! }
//!
//! #[tokio::test]
//! async fn test_custom_version() {
//!     // Specific sandbox version
//!     let sandbox = SandboxConfig::builder()
//!         .version("2.10.5")
//!         .fresh()
//!         .await;
//!     let near = Near::sandbox(&sandbox);
//! }
//!
//! #[tokio::test]
//! async fn test_custom_root() {
//!     // Custom root account name
//!     let sandbox = SandboxConfig::builder()
//!         .root_account("sb")
//!         .fresh()
//!         .await;
//!     let near = Near::sandbox(&sandbox);
//!     // Root account is now "sb" instead of "sandbox"
//! }
//! ```

use std::borrow::Cow;
use std::sync::Arc;

use testcontainers::{
    ContainerAsync, CopyToContainer, Image,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::sync::OnceCell;
use tracing::info;

use crate::client::Near;

// ============================================================================
// NearSandbox testcontainers Image (inlined from near-sandbox-testcontainer)
// ============================================================================

const DEFAULT_IMAGE: &str = "nearprotocol/sandbox";
const DEFAULT_VERSION: &str = "2.10.7";

/// The RPC port exposed by the NEAR sandbox container.
const RPC_PORT: ContainerPort = ContainerPort::Tcp(3030);

/// A [`testcontainers::Image`] for the NEAR sandbox Docker image
/// (`nearprotocol/sandbox`).
///
/// Configuration is passed via environment variables supported by the
/// image's entrypoint (`NEAR_ROOT_ACCOUNT`, `NEAR_CHAIN_ID`,
/// `NEAR_ENABLE_SANDBOX_LOG`).
///
/// The image name and tag can be overridden via the `NEAR_SANDBOX_IMAGE`
/// environment variable (e.g. `NEAR_SANDBOX_IMAGE=my-registry/sandbox:custom`).
/// The value is split on the last `:` into image name and tag (`name[:tag]`).
/// If no `:` is present, the value is used as the image name with the default tag.
#[derive(Debug, Clone)]
struct NearSandbox {
    image_name: String,
    tag: String,
    env_vars: Vec<(String, String)>,
    copy_to_sources: Vec<CopyToContainer>,
}

impl NearSandbox {
    /// Creates a new [`NearSandbox`] image with the given version tag.
    ///
    /// The image name and tag can be overridden by setting the
    /// `NEAR_SANDBOX_IMAGE` environment variable (e.g.
    /// `my-registry/sandbox:custom`).
    fn new(version: &str) -> Self {
        let (image_name, tag) = match std::env::var("NEAR_SANDBOX_IMAGE") {
            Ok(raw) => {
                let val = raw.trim();
                if val.is_empty() {
                    (DEFAULT_IMAGE.to_owned(), version.to_owned())
                } else {
                    match val.rsplit_once(':') {
                        Some((name, tag)) if !name.is_empty() && !tag.is_empty() => {
                            (name.to_owned(), tag.to_owned())
                        }
                        _ => (val.to_owned(), version.to_owned()),
                    }
                }
            }
            Err(_) => (DEFAULT_IMAGE.to_owned(), version.to_owned()),
        };

        Self {
            image_name,
            tag,
            env_vars: vec![
                // Enable sandbox logging for easier debugging
                ("NEAR_ENABLE_SANDBOX_LOG".to_owned(), "1".to_owned()),
            ],
            copy_to_sources: Vec::new(),
        }
    }

    fn with_account_id(mut self, account_id: impl Into<String>) -> Self {
        self.env_vars
            .push(("NEAR_ROOT_ACCOUNT".to_owned(), account_id.into()));
        self
    }

    fn with_chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.env_vars
            .push(("NEAR_CHAIN_ID".to_owned(), chain_id.into()));
        self
    }
}

impl Default for NearSandbox {
    fn default() -> Self {
        Self::new(DEFAULT_VERSION)
    }
}

impl Image for NearSandbox {
    fn name(&self) -> &str {
        &self.image_name
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        // Use the log message for readiness detection — it works across all
        // sandbox versions. Images >=2.11 also include a Docker HEALTHCHECK,
        // but we can't rely on it until we bump the minimum supported version.
        vec![WaitFor::message_on_stderr("Starting http server at")]
    }

    fn env_vars(
        &self,
    ) -> impl IntoIterator<Item = (impl Into<Cow<'_, str>>, impl Into<Cow<'_, str>>)> {
        self.env_vars.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    fn copy_to_sources(&self) -> impl IntoIterator<Item = &CopyToContainer> {
        &self.copy_to_sources
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[RPC_PORT]
    }
}

// ============================================================================
// Re-exports for convenience
// ============================================================================

// Re-export sandbox constants and trait from client module
pub use crate::client::{SANDBOX_ROOT_ACCOUNT, SANDBOX_ROOT_SECRET_KEY, SandboxNetwork};

// ============================================================================
// Global shared sandbox
// ============================================================================

/// Global shared sandbox instance.
static SHARED_SANDBOX: OnceCell<Sandbox> = OnceCell::const_new();

/// Stop the shared sandbox container on process exit.
///
/// Statics are never dropped in Rust, so the watchdog (which handles signals)
/// won't cover normal process exit. This `atexit` handler ensures the container
/// is cleaned up on clean termination.
///
/// Tokio's thread-local storage is already destroyed on the main thread by
/// the time `atexit` fires, so we spawn a new thread with fresh TLS.
extern "C" fn cleanup_shared_sandbox() {
    let handle = std::thread::spawn(|| {
        if let Some(sandbox) = SHARED_SANDBOX.get() {
            if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                let _ = rt.block_on(sandbox.container.stop_with_timeout(Some(0)));
            }
        }
    });
    let _ = handle.join();
}

/// Register atexit handler for shared sandbox cleanup.
fn register_shared_cleanup() {
    unsafe {
        libc::atexit(cleanup_shared_sandbox);
    }
}

// ============================================================================
// Sandbox
// ============================================================================

/// A sandbox instance backed by a Docker container via testcontainers.
///
/// Cloning a `Sandbox` shares the underlying container — the container is stopped
/// only when the last `Arc` reference is dropped.
///
/// Obtain one via [`SandboxConfig::shared()`] (global singleton) or
/// [`SandboxConfig::fresh()`] (new isolated instance).
///
/// # Cleanup
///
/// - **Fresh sandboxes**: cleaned up via testcontainers' `Drop` impl.
/// - **Shared sandbox**: cleaned up via an `atexit` handler (normal exit) and
///   the testcontainers `watchdog` (SIGINT/SIGTERM/SIGQUIT, e.g. Ctrl+C).
#[derive(Clone)]
pub struct Sandbox {
    #[allow(dead_code)]
    container: Arc<ContainerAsync<NearSandbox>>,
    rpc_url: String,
    root_account: String,
    chain_id: Option<String>,
}

impl Sandbox {
    async fn start(version: &str, root_account: String, chain_id: Option<String>) -> Self {
        info!(
            version = version,
            root_account = %root_account,
            chain_id = chain_id.as_deref().unwrap_or("(default)"),
            "Starting sandbox container"
        );

        let mut image = NearSandbox::new(version).with_account_id(&root_account);
        if let Some(ref id) = chain_id {
            image = image.with_chain_id(id);
        }
        let container = image
            .start()
            .await
            .expect("Failed to start sandbox container");

        let host = container
            .get_host()
            .await
            .expect("Failed to get sandbox host");

        let host_port = container
            .get_host_port_ipv4(RPC_PORT)
            .await
            .expect("Failed to get mapped port for sandbox RPC");

        let rpc_url = format!("http://{}:{}", host, host_port);

        info!(rpc_url = %rpc_url, "Sandbox container ready");

        Self {
            container: Arc::new(container),
            rpc_url,
            root_account,
            chain_id,
        }
    }

    /// Get a configured `Near` client for this sandbox.
    ///
    /// This is a convenience method equivalent to `Near::sandbox(self)`.
    pub fn client(&self) -> Near {
        Near::sandbox(self)
    }

    /// Set an account's balance in this sandbox.
    ///
    /// This patches the account's balance directly via the sandbox RPC,
    /// useful for testing scenarios that require specific balances
    /// (e.g., staking tests that need 1M+ NEAR).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_kit::*;
    /// use near_kit::sandbox::SandboxConfig;
    ///
    /// let sandbox = SandboxConfig::shared().await;
    ///
    /// // Set account balance to 1M NEAR for staking test
    /// sandbox.set_balance("validator.sandbox", NearToken::from_near(1_000_000)).await?;
    /// ```
    pub async fn set_balance(
        &self,
        account_id: impl Into<crate::AccountId>,
        balance: crate::NearToken,
    ) -> Result<(), crate::Error> {
        let near = self.client();
        let account_id: crate::AccountId = account_id.into();

        // Fetch raw account data from RPC - this includes all fields the sandbox expects
        let mut account_response: serde_json::Value = near
            .rpc()
            .call(
                "query",
                serde_json::json!({
                    "finality": "optimistic",
                    "request_type": "view_account",
                    "account_id": account_id.to_string()
                }),
            )
            .await
            .map_err(|e| crate::Error::Rpc(Box::new(e)))?;

        // Modify the amount field in the response
        if let Some(obj) = account_response.as_object_mut() {
            obj.insert(
                "amount".to_string(),
                serde_json::Value::String(balance.as_yoctonear().to_string()),
            );
        }

        let records = serde_json::json!([
            {
                "Account": {
                    "account_id": account_id.to_string(),
                    "account": account_response
                }
            }
        ]);

        near.rpc()
            .sandbox_patch_state(records)
            .await
            .map_err(|e| crate::Error::Rpc(Box::new(e)))
    }

    /// Fast-forward the sandbox by `delta_height` blocks.
    ///
    /// Useful for testing time-dependent logic (e.g., lockups, staking epoch
    /// changes) without waiting for real block production.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use near_kit::sandbox::SandboxConfig;
    ///
    /// let sandbox = SandboxConfig::shared().await;
    /// sandbox.fast_forward(1000).await?;
    /// ```
    pub async fn fast_forward(&self, delta_height: u64) -> Result<(), crate::Error> {
        self.client()
            .rpc()
            .sandbox_fast_forward(delta_height)
            .await?;
        Ok(())
    }
}

impl SandboxNetwork for Sandbox {
    fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    fn root_account_id(&self) -> &str {
        &self.root_account
    }

    fn root_secret_key(&self) -> &str {
        SANDBOX_ROOT_SECRET_KEY
    }

    fn chain_id(&self) -> Option<&str> {
        self.chain_id.as_deref()
    }
}

// ============================================================================
// SandboxConfig
// ============================================================================

/// Configuration and factory for sandbox instances.
///
/// Provides two main ways to get a sandbox:
/// - [`SandboxConfig::shared()`] - Returns a reference to a global singleton sandbox
/// - [`SandboxConfig::fresh()`] - Spawns a new sandbox that is stopped when the last clone drops
///
/// For custom configuration (e.g., specific version or root account), use [`SandboxConfig::builder()`].
///
/// # Example
///
/// ```rust,ignore
/// use near_kit::*;
/// use near_kit::sandbox::SandboxConfig;
///
/// #[tokio::test]
/// async fn test_shared() {
///     // Fast - reuses existing sandbox
///     let near = Near::sandbox(SandboxConfig::shared().await);
/// }
///
/// #[tokio::test]
/// async fn test_fresh() {
///     // Isolated - gets its own sandbox
///     let sandbox = SandboxConfig::fresh().await;
///     let near = Near::sandbox(&sandbox);
/// }
///
/// #[tokio::test]
/// async fn test_custom_version() {
///     let sandbox = SandboxConfig::builder()
///         .version("2.10.5")
///         .fresh()
///         .await;
///     let near = Near::sandbox(&sandbox);
/// }
/// ```
pub struct SandboxConfig;

impl SandboxConfig {
    /// Get a reference to the global shared sandbox.
    ///
    /// The first call will start the sandbox; subsequent calls return
    /// the same instance. This is ideal for most tests where you don't
    /// need completely fresh blockchain state.
    ///
    /// The shared sandbox is never stopped - it persists for the entire
    /// test run.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let near = Near::sandbox(SandboxConfig::shared().await);
    /// // or
    /// let near = SandboxConfig::shared().await.client();
    /// ```
    pub async fn shared() -> &'static Sandbox {
        SHARED_SANDBOX
            .get_or_init(|| async {
                let sandbox =
                    Sandbox::start(DEFAULT_VERSION, SANDBOX_ROOT_ACCOUNT.to_string(), None).await;
                register_shared_cleanup();
                sandbox
            })
            .await
    }

    /// Spawn a fresh sandbox instance.
    ///
    /// Creates a new sandbox with clean state. The sandbox container will be
    /// stopped and cleaned up when the last clone of the returned [`Sandbox`]
    /// is dropped.
    ///
    /// Use this for tests that need guaranteed isolation from other tests.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::fresh().await;
    /// let near = Near::sandbox(&sandbox);
    /// // or
    /// let near = sandbox.client();
    /// // sandbox is stopped when last reference goes out of scope
    /// ```
    pub async fn fresh() -> Sandbox {
        Sandbox::start(DEFAULT_VERSION, SANDBOX_ROOT_ACCOUNT.to_string(), None).await
    }

    /// Create a builder for custom sandbox configuration.
    ///
    /// Use this when you need to specify a particular sandbox version
    /// or custom root account name.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::builder()
    ///     .version("2.10.5")
    ///     .root_account("sb")
    ///     .fresh()
    ///     .await;
    /// ```
    pub fn builder() -> SandboxBuilder {
        SandboxBuilder::new()
    }
}

// ============================================================================
// SandboxBuilder
// ============================================================================

/// Builder for custom sandbox configuration.
///
/// Created via [`SandboxConfig::builder()`].
pub struct SandboxBuilder {
    version: Option<String>,
    root_account: Option<String>,
    chain_id: Option<String>,
}

impl SandboxBuilder {
    fn new() -> Self {
        Self {
            version: None,
            root_account: None,
            chain_id: None,
        }
    }

    /// Set the sandbox version to use.
    ///
    /// If not specified, uses the default version (`2.10.7`).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::builder()
    ///     .version("2.10.5")
    ///     .fresh()
    ///     .await;
    /// ```
    pub fn version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set the root account name for this sandbox.
    ///
    /// If not specified, defaults to `"sandbox"`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::builder()
    ///     .root_account("sb")
    ///     .fresh()
    ///     .await;
    /// // Sub-accounts will be "alice.sb" instead of "alice.sandbox"
    /// ```
    pub fn root_account(mut self, name: impl crate::TryIntoAccountId) -> Self {
        let account_id = name
            .try_into_account_id()
            .expect("invalid sandbox root account name");
        self.root_account = Some(account_id.to_string());
        self
    }

    /// Set the chain ID for this sandbox.
    ///
    /// Useful for testing chain-ID-dependent logic (e.g., signed wallet
    /// requests that compare against the chain ID).
    ///
    /// If not specified, the sandbox uses its default chain ID.
    ///
    /// **Note:** `"mainnet"` and `"testnet"` are not supported — the
    /// `near-sandbox` binary refuses to run with these chain IDs.
    /// Use custom chain IDs instead (e.g., `"pinet"` for Private Shard).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::builder()
    ///     .chain_id("pinet")
    ///     .fresh()
    ///     .await;
    /// ```
    pub fn chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = Some(chain_id.into());
        self
    }

    /// Spawn a fresh sandbox with the configured options.
    ///
    /// The sandbox container will be stopped when the last clone of
    /// the returned [`Sandbox`] is dropped.
    pub async fn fresh(self) -> Sandbox {
        let version = self.version.as_deref().unwrap_or(DEFAULT_VERSION);
        let root_account = self
            .root_account
            .unwrap_or_else(|| SANDBOX_ROOT_ACCOUNT.to_string());
        Sandbox::start(version, root_account, self.chain_id).await
    }

    /// Get or create the shared sandbox with the configured options.
    ///
    /// **Note:** The version, root account, and chain ID are only used if the
    /// shared sandbox hasn't been initialized yet. If it's already running, the
    /// existing instance is returned regardless of the options specified here.
    pub async fn shared(self) -> &'static Sandbox {
        let version = self.version;
        let root_account = self.root_account;
        let chain_id = self.chain_id;

        SHARED_SANDBOX
            .get_or_init(|| async {
                let v = version.as_deref().unwrap_or(DEFAULT_VERSION);
                let r = root_account.unwrap_or_else(|| SANDBOX_ROOT_ACCOUNT.to_string());
                let sandbox = Sandbox::start(v, r, chain_id).await;
                register_shared_cleanup();
                sandbox
            })
            .await
    }
}
