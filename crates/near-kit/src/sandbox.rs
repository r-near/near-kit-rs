//! Sandbox testing utilities for NEAR Protocol.
//!
//! This module provides ergonomic APIs for testing against a local NEAR sandbox.
//! It handles sandbox lifecycle management, including shared singleton instances
//! for test performance and fresh instances for isolated tests.
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
//!     // Fresh sandbox - completely isolated, stopped when last clone drops
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

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::OnceCell;
use tracing::{debug, info};

use crate::client::Near;

// ============================================================================
// Logging and metrics
// ============================================================================

/// Counter for sandbox instances created (for debugging/testing)
static SANDBOX_INSTANCE_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Get the total number of sandbox instances created during this process.
///
/// Useful for verifying that shared sandboxes are actually being shared.
pub fn sandbox_instance_count() -> usize {
    SANDBOX_INSTANCE_COUNT.load(Ordering::Relaxed)
}

// ============================================================================
// Re-exports for convenience
// ============================================================================

/// Root account ID for the sandbox ("sandbox").
pub const ROOT_ACCOUNT: &str = "sandbox";

/// Root account secret key for the sandbox.
pub const ROOT_SECRET_KEY: &str = "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

// Re-export SandboxNetwork trait
pub use crate::client::SandboxNetwork;

// ============================================================================
// Global shared sandbox
// ============================================================================

/// Global shared sandbox instance.
static SHARED_SANDBOX: OnceCell<Sandbox> = OnceCell::const_new();

// ============================================================================
// Sandbox
// ============================================================================

/// A sandbox instance backed by `Arc<near_sandbox::Sandbox>`.
///
/// Cloning a `Sandbox` shares the underlying process — the sandbox is stopped
/// only when the last `Arc` reference is dropped.
///
/// Obtain one via [`SandboxConfig::shared()`] (global singleton) or
/// [`SandboxConfig::fresh()`] (new isolated instance).
///
/// # Note on Cleanup
///
/// The sandbox process is killed when the last reference drops.
/// For the shared singleton returned by [`SandboxConfig::shared()`],
/// the process may outlive the test process since Rust doesn't run
/// static destructors.
#[derive(Clone)]
pub struct Sandbox {
    inner: Arc<near_sandbox::Sandbox>,
    root_account: String,
}

impl Sandbox {
    async fn start(version: &str, root_account: String) -> Self {
        let instance_num = SANDBOX_INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        info!(
            instance = instance_num,
            version = version,
            root_account = %root_account,
            "Starting sandbox"
        );

        let inner = near_sandbox::Sandbox::start_sandbox_with_version(version)
            .await
            .expect("Failed to start sandbox");

        info!(
            instance = instance_num,
            rpc_url = %inner.rpc_addr,
            "Sandbox ready"
        );

        Self {
            inner: Arc::new(inner),
            root_account,
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
            .map_err(crate::Error::Rpc)?;

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
            .map_err(crate::Error::Rpc)
    }
}

impl SandboxNetwork for Sandbox {
    fn rpc_url(&self) -> &str {
        &self.inner.rpc_addr
    }

    fn root_account_id(&self) -> &str {
        &self.root_account
    }

    fn root_secret_key(&self) -> &str {
        ROOT_SECRET_KEY
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Arc handles the actual cleanup — log when we're the last reference
        if Arc::strong_count(&self.inner) == 1 {
            debug!(rpc_url = %self.inner.rpc_addr, "Stopping sandbox (last reference dropped)");
        }
    }
}

// ============================================================================
// Backward-compatible type aliases
// ============================================================================

/// Deprecated: use [`Sandbox`] instead.
pub type SharedSandbox = Sandbox;

/// Deprecated: use [`Sandbox`] instead.
pub type OwnedSandbox = Sandbox;

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
            .get_or_init(|| {
                Sandbox::start(
                    near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION,
                    ROOT_ACCOUNT.to_string(),
                )
            })
            .await
    }

    /// Spawn a fresh sandbox instance.
    ///
    /// Creates a new sandbox with clean state. The sandbox process will be
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
        Sandbox::start(
            near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION,
            ROOT_ACCOUNT.to_string(),
        )
        .await
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
}

impl SandboxBuilder {
    fn new() -> Self {
        Self {
            version: None,
            root_account: None,
        }
    }

    /// Set the sandbox version to use.
    ///
    /// If not specified, uses the default version from `near-sandbox`.
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
    pub fn root_account(mut self, name: impl Into<String>) -> Self {
        self.root_account = Some(name.into());
        self
    }

    /// Spawn a fresh sandbox with the configured options.
    ///
    /// The sandbox process will be stopped when the last clone of
    /// the returned [`Sandbox`] is dropped.
    pub async fn fresh(self) -> Sandbox {
        let version = self
            .version
            .as_deref()
            .unwrap_or(near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION);
        let root_account = self
            .root_account
            .unwrap_or_else(|| ROOT_ACCOUNT.to_string());
        Sandbox::start(version, root_account).await
    }

    /// Get or create the shared sandbox with the configured options.
    ///
    /// **Note:** The version and root account are only used if the shared
    /// sandbox hasn't been initialized yet. If it's already running, the
    /// existing instance is returned regardless of the options specified here.
    pub async fn shared(self) -> &'static Sandbox {
        let version = self.version;
        let root_account = self.root_account;

        SHARED_SANDBOX
            .get_or_init(|| {
                let v = version
                    .as_deref()
                    .unwrap_or(near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION);
                let r = root_account.unwrap_or_else(|| ROOT_ACCOUNT.to_string());
                Sandbox::start(v, r)
            })
            .await
    }
}
