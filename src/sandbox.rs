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
//! use near_kit::prelude::*;
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
//!     // Fresh sandbox - completely isolated, stopped on drop
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
//! ```

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
pub const ROOT_SECRET_KEY: &str =
    "ed25519:3tgdk2wPraJzT4nsTuf86UX41xgPNk3MHnq8epARMdBNs29AFEztAuaQ7iHddDfXG9F2RzV1XNQYgJyAyoW51UBB";

// Re-export SandboxNetwork trait
pub use crate::client::SandboxNetwork;

// ============================================================================
// Global shared sandbox
// ============================================================================

/// Global shared sandbox instance.
static SHARED_SANDBOX: OnceCell<SharedSandbox> = OnceCell::const_new();

// ============================================================================
// SharedSandbox
// ============================================================================

/// A shared sandbox instance that persists across tests.
///
/// This wrapper holds a `near_sandbox::Sandbox` and implements [`SandboxNetwork`].
/// It is used by [`SandboxConfig::shared()`] to provide a singleton sandbox.
///
/// The shared sandbox is never stopped - it persists for the entire test run.
pub struct SharedSandbox {
    inner: near_sandbox::Sandbox,
}

impl SharedSandbox {
    async fn init() -> Self {
        Self::init_with_version(near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION).await
    }

    async fn init_with_version(version: &str) -> Self {
        let instance_num = SANDBOX_INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        info!(
            instance = instance_num,
            version = version,
            mode = "shared",
            "Starting sandbox"
        );

        let inner = near_sandbox::Sandbox::start_sandbox_with_version(version)
            .await
            .expect("Failed to start shared sandbox");

        info!(
            instance = instance_num,
            rpc_url = %inner.rpc_addr,
            "Shared sandbox ready"
        );

        Self { inner }
    }

    /// Get a configured `Near` client for this sandbox.
    ///
    /// This is a convenience method equivalent to `Near::sandbox(self)`.
    pub fn client(&self) -> Near {
        Near::sandbox(self)
    }
}

impl SandboxNetwork for SharedSandbox {
    fn rpc_url(&self) -> &str {
        &self.inner.rpc_addr
    }

    fn root_account_id(&self) -> &str {
        ROOT_ACCOUNT
    }

    fn root_secret_key(&self) -> &str {
        ROOT_SECRET_KEY
    }
}

// ============================================================================
// OwnedSandbox
// ============================================================================

/// An owned sandbox instance that stops when dropped.
///
/// This wrapper holds a `near_sandbox::Sandbox` and implements [`SandboxNetwork`].
/// When dropped, it will stop the sandbox process and clean up resources.
///
/// Use [`SandboxConfig::fresh()`] to create a fresh sandbox for tests that need
/// completely isolated state.
pub struct OwnedSandbox {
    inner: Option<near_sandbox::Sandbox>,
}

impl OwnedSandbox {
    async fn spawn() -> Self {
        Self::spawn_with_version(near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION).await
    }

    async fn spawn_with_version(version: &str) -> Self {
        let instance_num = SANDBOX_INSTANCE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        info!(
            instance = instance_num,
            version = version,
            mode = "fresh",
            "Starting sandbox"
        );

        let inner = near_sandbox::Sandbox::start_sandbox_with_version(version)
            .await
            .expect("Failed to start sandbox");

        info!(
            instance = instance_num,
            rpc_url = %inner.rpc_addr,
            "Fresh sandbox ready"
        );

        Self { inner: Some(inner) }
    }

    /// Get a configured `Near` client for this sandbox.
    ///
    /// This is a convenience method equivalent to `Near::sandbox(self)`.
    pub fn client(&self) -> Near {
        Near::sandbox(self)
    }
}

impl SandboxNetwork for OwnedSandbox {
    fn rpc_url(&self) -> &str {
        &self
            .inner
            .as_ref()
            .expect("sandbox already stopped")
            .rpc_addr
    }

    fn root_account_id(&self) -> &str {
        ROOT_ACCOUNT
    }

    fn root_secret_key(&self) -> &str {
        ROOT_SECRET_KEY
    }
}

impl Drop for OwnedSandbox {
    fn drop(&mut self) {
        if let Some(sandbox) = self.inner.take() {
            debug!(rpc_url = %sandbox.rpc_addr, "Stopping fresh sandbox");
            // The near_sandbox::Sandbox will kill the child process when dropped
            drop(sandbox);
        }
    }
}

// ============================================================================
// SandboxConfig
// ============================================================================

/// Configuration and factory for sandbox instances.
///
/// Provides two main ways to get a sandbox:
/// - [`SandboxConfig::shared()`] - Returns a reference to a global singleton sandbox
/// - [`SandboxConfig::fresh()`] - Spawns a new sandbox that is stopped on drop
///
/// For custom configuration (e.g., specific version), use [`SandboxConfig::builder()`].
///
/// # Example
///
/// ```rust,ignore
/// use near_kit::prelude::*;
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
    pub async fn shared() -> &'static SharedSandbox {
        SHARED_SANDBOX.get_or_init(SharedSandbox::init).await
    }

    /// Spawn a fresh sandbox instance.
    ///
    /// Creates a new sandbox with clean state. The sandbox will be
    /// stopped and cleaned up when the returned [`OwnedSandbox`] is dropped.
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
    /// // sandbox is stopped when it goes out of scope
    /// ```
    pub async fn fresh() -> OwnedSandbox {
        OwnedSandbox::spawn().await
    }

    /// Create a builder for custom sandbox configuration.
    ///
    /// Use this when you need to specify a particular sandbox version
    /// or other advanced options.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let sandbox = SandboxConfig::builder()
    ///     .version("2.10.5")
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
}

impl SandboxBuilder {
    fn new() -> Self {
        Self { version: None }
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

    /// Spawn a fresh sandbox with the configured options.
    ///
    /// The sandbox will be stopped when the returned [`OwnedSandbox`] is dropped.
    pub async fn fresh(self) -> OwnedSandbox {
        let version = self
            .version
            .as_deref()
            .unwrap_or(near_sandbox::DEFAULT_NEAR_SANDBOX_VERSION);
        OwnedSandbox::spawn_with_version(version).await
    }

    /// Get or create the shared sandbox with the configured options.
    ///
    /// **Note:** The version is only used if the shared sandbox hasn't been
    /// initialized yet. If it's already running, the existing instance is
    /// returned regardless of the version specified here.
    pub async fn shared(self) -> &'static SharedSandbox {
        // If a version is specified and sandbox isn't initialized yet,
        // initialize with that version
        if let Some(version) = self.version {
            SHARED_SANDBOX
                .get_or_init(|| SharedSandbox::init_with_version(&version))
                .await
        } else {
            SHARED_SANDBOX.get_or_init(SharedSandbox::init).await
        }
    }
}
