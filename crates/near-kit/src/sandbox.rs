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
    ContainerAsync, CopyToContainer, Image, ImageExt,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};
use tokio::sync::OnceCell;
use tracing::info;

use crate::client::Near;

// ============================================================================
// NearSandbox testcontainers Image (inlined from near-sandbox-testcontainer)
// ============================================================================

const DEFAULT_VERSION: &str = "2.10.7";

/// The RPC port exposed by the NEAR sandbox container.
const RPC_PORT: ContainerPort = ContainerPort::Tcp(3030);

/// A [`testcontainers::Image`] for the NEAR sandbox Docker image
/// (`nearprotocol/sandbox`).
#[derive(Debug, Clone)]
struct NearSandbox {
    tag: String,
    env_vars: Vec<(String, String)>,
    copy_to_sources: Vec<CopyToContainer>,
    /// Account ID for the root/validator account.
    account_id: String,
    /// Seed for deterministic key generation (passed to `--test-seed`).
    test_seed: String,
    /// Chain ID for the sandbox (passed to `--chain-id`).
    chain_id: Option<String>,
}

impl NearSandbox {
    /// Creates a new [`NearSandbox`] image with the given version tag.
    fn new(version: &str) -> Self {
        Self {
            tag: version.to_owned(),
            env_vars: vec![
                // Enable logging so we can detect the ready condition
                ("NEAR_ENABLE_SANDBOX_LOG".to_owned(), "1".to_owned()),
            ],
            copy_to_sources: Vec::new(),
            account_id: SANDBOX_ROOT_ACCOUNT.to_owned(),
            test_seed: "sandbox".to_owned(),
            chain_id: None,
        }
    }

    fn with_account_id(mut self, account_id: impl Into<String>) -> Self {
        self.account_id = account_id.into();
        self
    }

    fn with_chain_id(mut self, chain_id: impl Into<String>) -> Self {
        self.chain_id = Some(chain_id.into());
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
        "nearprotocol/sandbox"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr("Starting http server at")]
    }

    fn entrypoint(&self) -> Option<&str> {
        Some("/bin/sh")
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<Cow<'_, str>>> {
        // Override the default entrypoint to use --test-seed for deterministic keys
        // and --account-id for custom root account names
        let chain_id_flag = match &self.chain_id {
            Some(id) => format!(" --chain-id {id}"),
            None => String::new(),
        };
        let script = format!(
            concat!(
                "export RUST_LOG=\"neard::cli=off,info\" && ",
                "near-sandbox --home /data init --fast ",
                "--test-seed {test_seed} --account-id {account_id}{chain_id_flag} && ",
                "near-sandbox --home /data run ",
                "--rpc-addr 0.0.0.0:3030 --network-addr 0.0.0.0:3031"
            ),
            test_seed = self.test_seed,
            account_id = self.account_id,
            chain_id_flag = chain_id_flag,
        );
        vec!["-c".to_string(), script]
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
/// Tokio's thread-local storage is already destroyed on the main thread by
/// the time `atexit` fires, so `Runtime::block_on` panics there. We work
/// around this by spawning a new thread (which gets fresh TLS) and calling
/// `stop_with_timeout` via a temporary tokio runtime.
///
/// The container is started with `auto_remove = true`, so Docker removes it
/// automatically after it stops.
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
/// All containers are started with `auto_remove = true` so Docker removes them
/// after they stop. For fresh sandboxes, testcontainers' `Drop` handles stopping.
/// For the shared sandbox, an `atexit` handler stops the container via the
/// testcontainers API on a spawned thread (to avoid tokio TLS destruction).
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
            .with_host_config_modifier(|hc| {
                hc.auto_remove = Some(true);
            })
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
    pub fn root_account(mut self, name: &str) -> Self {
        // Validate by parsing as AccountId
        let _: crate::AccountId = name.parse().expect("invalid sandbox root account name");
        self.root_account = Some(name.to_string());
        self
    }

    /// Set the chain ID for this sandbox.
    ///
    /// Useful for testing against a sandbox that mimics a specific network
    /// (e.g., `"mainnet"`) so that chain-ID-dependent logic behaves correctly.
    ///
    /// If not specified, the sandbox uses its default chain ID.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::builder()
    ///     .chain_id("mainnet")
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
