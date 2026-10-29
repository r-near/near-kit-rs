//! A local NEAR node in Docker, for tests that use [`near-kit`](https://docs.rs/near-kit).
//!
//! [`Sandbox`] starts a `nearprotocol/sandbox` container through
//! testcontainers and hands back an ordinary `near_kit::SigningNear` for its
//! root account, so test code reads like production code. Accounts are
//! created the normal way, with `Action::create_account()` transactions from
//! the root account.
//!
//! ## Requirements
//!
//! Docker must be installed and running. testcontainers finds it through the
//! default socket; when Docker listens elsewhere (Colima, Rancher Desktop and
//! similar setups), set `DOCKER_HOST` to the active context's endpoint:
//!
//! ```sh
//! export DOCKER_HOST="$(docker context inspect --format '{{.Endpoints.docker.Host}}')"
//! ```
//!
//! The first start pulls the image, which can take a while.
//!
//! ## Starting a sandbox
//!
//! - [`Sandbox::builder().shared()`](SandboxBuilder::shared) starts one
//!   process-wide sandbox on first use and returns the same `&'static Sandbox`
//!   afterwards. Tests sharing it must use distinct account names. It is
//!   removed when the process exits.
//! - [`Sandbox::builder().fresh()`](SandboxBuilder::fresh) starts a new
//!   sandbox with clean state, stopped when the last clone of it drops.
//!
//! [`Sandbox::root_account`] is a `SigningNear` for the root account
//! (`sandbox` unless [`root_account`](SandboxBuilder::root_account) sets
//! another), funded at genesis. Sign as another account with
//! `root.near().with_signer(signer)`, which reuses the connection.
//!
//! ```rust,no_run
//! use near_kit::protocol::Action;
//! use near_kit::signer::{InMemorySigner, SecretKey};
//! use near_kit::{NearToken, account};
//! use near_kit_sandbox::Sandbox;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let sandbox = Sandbox::builder().shared().await?;
//! let root = sandbox.root_account();
//!
//! // Create and fund an account with its own key.
//! let alice_key = SecretKey::generate_ed25519();
//! root.transaction(account!("alice.sandbox"))
//!     .add_action(Action::create_account())
//!     .add_action(Action::transfer(NearToken::from_near(100)))
//!     .add_action(Action::add_full_access_key(alice_key.public_key()))
//!     .await?
//!     .result()?;
//!
//! let alice = root
//!     .near()
//!     .with_signer(InMemorySigner::new(account!("alice.sandbox"), alice_key));
//! let balance = alice.near().balance(alice.account_id()).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Changing chain state directly
//!
//! - [`Sandbox::set_balance`] sets an account's balance and returns once the
//!   new balance is visible.
//! - [`Sandbox::patch_state`] writes raw state records (accounts, access keys,
//!   contract code or storage). The patch applies asynchronously, so poll for
//!   the state you wrote before relying on it.
//! - [`Sandbox::fast_forward`] moves the chain ahead by a number of blocks,
//!   for code that depends on block height or epochs.
//!
//! ```rust,no_run
//! use near_kit::{NearToken, account};
//! use near_kit_sandbox::Sandbox;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let sandbox = Sandbox::builder().fresh().await?;
//! sandbox
//!     .set_balance(account!("sandbox"), NearToken::from_near(1_000_000))
//!     .await?;
//! sandbox.fast_forward(1_000).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Versions and configuration
//!
//! The builder picks the nearcore release to run with
//! [`version`](SandboxBuilder::version) (by default the release this crate is
//! tested against), the root account with
//! [`root_account`](SandboxBuilder::root_account), and the chain ID with
//! [`chain_id`](SandboxBuilder::chain_id). `"mainnet"` and `"testnet"` are
//! rejected as chain IDs, because neard reserves them. Options passed to
//! `shared()` apply only to the call that starts the shared sandbox.
//!
//! ```rust,no_run
//! use near_kit::account;
//! use near_kit_sandbox::Sandbox;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let sandbox = Sandbox::builder()
//!     .version("2.13.4")
//!     .root_account(account!("local"))
//!     .chain_id("localnet")
//!     .fresh()
//!     .await?;
//! assert_eq!(sandbox.root_account().account_id(), "local");
//! # Ok(())
//! # }
//! ```
//!
//! Set `NEAR_SANDBOX_IMAGE` to use another image, as `name` or `name:tag`
//! (for example `my-registry/near-sandbox:custom`). The image must define a
//! Docker health check, which is how the crate knows the node is ready.
//!
//! ## Features
//!
//! - `tracing`: sandbox lifecycle events, and near-kit's `tracing` feature.
//! - `integration-tests`: this repository's own Docker-backed tests and
//!   examples. Applications don't need it.

#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Compiles the README's code blocks as doctests.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use near_kit::AccountIdRef;
use near_kit::protocol::ChainId;
use near_kit::rpc::RpcError;
use near_kit::signer::{InMemorySigner, SecretKey, Signer};
use near_kit::{AccountId, Error, Near, NearToken, SigningNear};
use testcontainers::{
    ContainerAsync, Healthcheck, Image, ImageExt, TestcontainersError,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};
use thiserror::Error as ThisError;
use tokio::sync::OnceCell;

#[cfg(feature = "tracing")]
use tracing::info;

#[cfg(not(feature = "tracing"))]
macro_rules! info {
    ($($arg:tt)*) => {};
}

/// Default sandbox root account ID.
pub const SANDBOX_ROOT_ACCOUNT: &AccountIdRef = AccountIdRef::new_or_panic("sandbox");

/// Default sandbox root secret key.
///
/// This deterministic key is generated by `near-sandbox init --test-seed sandbox`.
pub const SANDBOX_ROOT_SECRET_KEY: &str = "ed25519:3JoAjwLppjgvxkk6kNsu5wQj3FfUJnpBKWieC73hVTpBeA6FZiCc5tfyZL3a3tHeQJegQe4qGSv8FLsYp7TYd1r6";

/// Failure while validating sandbox configuration or starting its container.
#[derive(Debug, ThisError)]
pub enum SandboxError {
    /// The requested chain ID is reserved for a public network.
    #[error("sandbox chain ID {0:?} is reserved by neard; use a custom ID such as \"localnet\"")]
    UnsupportedChainId(String),

    /// Docker or testcontainers failed while preparing the sandbox.
    #[error("sandbox container {operation} failed: {message}")]
    Container {
        /// The container operation that failed.
        operation: &'static str,
        /// The underlying error message.
        message: String,
    },

    /// A sandbox-specific RPC request failed.
    #[error("sandbox RPC request failed: {0}")]
    Rpc(#[source] Box<RpcError>),

    /// A patched account balance did not become observable within the retry budget.
    #[error("sandbox balance patch for {account_id} was not observable after {attempts} attempts")]
    PatchTimeout {
        /// The account whose patched balance was not observed.
        account_id: AccountId,
        /// Number of observation attempts made.
        attempts: u32,
        /// Last client error observed while polling, if any.
        #[source]
        last_error: Option<Box<Error>>,
    },
}

impl From<RpcError> for SandboxError {
    fn from(error: RpcError) -> Self {
        Self::Rpc(Box::new(error))
    }
}

// ============================================================================
// NearSandbox testcontainers Image (inlined from near-sandbox-testcontainer)
// ============================================================================

const DEFAULT_IMAGE: &str = "nearprotocol/sandbox";
// nearcore 2.14 (protocol v88).
const DEFAULT_VERSION: &str = "2.14.0";

/// The RPC port exposed by the NEAR sandbox container.
const RPC_PORT: ContainerPort = ContainerPort::Tcp(3030);

/// Grace period before Docker starts counting failed healthcheck probes.
///
/// The image's own healthcheck (10s start period, 5s interval, 3 retries)
/// flips the container to `unhealthy` after roughly 25–30s without a
/// successful `/status` probe. That is too tight on a loaded Docker host —
/// e.g. several sandbox suites starting containers at once — where neard can
/// take longer than that just to initialize, and testcontainers then fails the
/// start. A successful probe still marks the container healthy immediately, so
/// a longer grace period only delays reporting a node that never comes up.
/// The probe command, interval and retries are inherited from the image.
const HEALTHCHECK_START_PERIOD: Duration = Duration::from_secs(90);

/// Overall budget for the container to become healthy (testcontainers defaults
/// to 60s). Kept above [`HEALTHCHECK_START_PERIOD`] plus the image's
/// `retries × interval` so a node that never comes up is reported as
/// `Unhealthy` rather than as a bare startup timeout.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(120);

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
///
/// The image must define a Docker `HEALTHCHECK` for readiness detection.
#[derive(Debug, Clone)]
struct NearSandbox {
    image_name: String,
    tag: String,
    env_vars: Vec<(&'static str, String)>,
}

impl NearSandbox {
    /// The image for `version`, unless `NEAR_SANDBOX_IMAGE` names another.
    fn new(version: &str, root_account: &AccountId, chain_id: Option<&ChainId>) -> Self {
        let image = std::env::var("NEAR_SANDBOX_IMAGE").ok();
        let (image_name, tag) = match image.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            None => (DEFAULT_IMAGE, version),
            Some(image) => match image.rsplit_once(':') {
                Some((name, tag)) if !name.is_empty() && !tag.is_empty() => (name, tag),
                _ => (image, version),
            },
        };
        let mut env_vars = vec![
            // Enable sandbox logging for easier debugging
            ("NEAR_ENABLE_SANDBOX_LOG", "1".to_owned()),
            ("NEAR_ROOT_ACCOUNT", root_account.to_string()),
        ];
        env_vars.extend(chain_id.map(|c| ("NEAR_CHAIN_ID", c.to_string())));
        Self {
            image_name: image_name.to_owned(),
            tag: tag.to_owned(),
            env_vars,
        }
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
        vec![WaitFor::healthcheck()]
    }

    fn env_vars(
        &self,
    ) -> impl IntoIterator<Item = (impl Into<Cow<'_, str>>, impl Into<Cow<'_, str>>)> {
        self.env_vars.iter().map(|(k, v)| (*k, v.as_str()))
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[RPC_PORT]
    }
}

// ============================================================================
// Global shared sandbox
// ============================================================================

/// Global shared sandbox instance.
static SHARED_SANDBOX: OnceCell<Sandbox> = OnceCell::const_new();

/// Remove the shared sandbox container on normal process exit.
///
/// Statics are never dropped in Rust, so the watchdog (which handles signals)
/// won't cover normal process exit. This `atexit` handler ensures the container
/// is cleaned up on clean termination.
///
/// We must NOT use tokio/async here: by the time `atexit` fires the main
/// thread's TLS (including tokio's) is destroyed, and reusing the async Docker
/// client panics with a TLS `AccessError`. Such a panic inside this
/// non-unwinding `extern "C"` fn would abort the process, so we shell out to
/// `docker rm` (which touches no Rust/tokio TLS) and wrap the body in
/// `catch_unwind` as a belt-and-suspenders guard against any unwind crossing
/// the C boundary. Removal is best-effort and errors are ignored; the
/// testcontainers watchdog still covers signal-based termination (Ctrl+C etc.).
extern "C" fn cleanup_shared_sandbox() {
    let _ = std::panic::catch_unwind(|| {
        if let Some(sandbox) = SHARED_SANDBOX.get() {
            // `OnceCell::get` on a `tokio::sync::OnceCell` is a non-async atomic
            // check, so it is safe to call from an `atexit` handler.
            let _ = std::process::Command::new("docker")
                .args(["rm", "-f", "-v", &sandbox.container_id])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });
}

// ============================================================================
// Sandbox
// ============================================================================

/// A sandbox instance backed by a Docker container via testcontainers.
///
/// Cloning a `Sandbox` shares the underlying container — the container is stopped
/// only when the last `Arc` reference is dropped.
///
/// Obtain one via `Sandbox::builder().shared()` (global singleton) or
/// `Sandbox::builder().fresh()` (new isolated instance); see
/// [`Sandbox::builder`].
///
/// # Cleanup
///
/// - **Fresh sandboxes**: cleaned up via testcontainers' `Drop` impl.
/// - **Shared sandbox**: cleaned up via an `atexit` handler that synchronously
///   shells out to `docker rm -f` by container id (normal exit; no tokio/async)
///   and the testcontainers `watchdog` (SIGINT/SIGTERM/SIGQUIT, e.g. Ctrl+C).
#[derive(Clone)]
pub struct Sandbox {
    _container: Arc<ContainerAsync<NearSandbox>>,
    /// Docker container id, captured at start so the `atexit` handler can remove
    /// the shared container synchronously without touching the async client.
    container_id: String,
    rpc_url: String,
    chain_id: ChainId,
    root_signer: Arc<dyn Signer>,
}

impl Sandbox {
    /// A builder for a sandbox: `.fresh()` for a new isolated instance,
    /// `.shared()` for the process-wide one, after any
    /// [`version`](SandboxBuilder::version),
    /// [`root_account`](SandboxBuilder::root_account) or
    /// [`chain_id`](SandboxBuilder::chain_id).
    ///
    /// - [`shared`](SandboxBuilder::shared) starts the sandbox on first use and
    ///   returns the same `&'static Sandbox` afterwards. It is never stopped
    ///   during the run, which suits most tests.
    /// - [`fresh`](SandboxBuilder::fresh) starts one with clean state, stopped
    ///   when the last clone of the returned [`Sandbox`] drops.
    ///
    /// ```rust,no_run
    /// use near_kit::account;
    /// use near_kit_sandbox::Sandbox;
    ///
    /// # async fn example() -> Result<(), near_kit_sandbox::SandboxError> {
    /// let root = Sandbox::builder().shared().await?.root_account();
    ///
    /// let sandbox = Sandbox::builder()
    ///     .version("2.13.4")
    ///     .root_account(account!("sb"))
    ///     .fresh()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> SandboxBuilder {
        SandboxBuilder {
            version: None,
            root_account: None,
            chain_id: None,
            construction_error: None,
        }
    }

    /// Get a signing client for this sandbox's root account.
    ///
    /// The root account is funded at genesis, so it can create and fund
    /// test accounts. For reads use `root_account().near()`; for another account,
    /// `root_account().near().with_signer(signer)`.
    pub fn root_account(&self) -> SigningNear {
        self.near().with_signer(self.root_signer.clone())
    }

    /// A read-only client for this sandbox's RPC endpoint.
    fn near(&self) -> Near {
        Near::custom(&self.rpc_url, self.chain_id.clone()).build()
    }

    /// Set an account's balance in this sandbox.
    ///
    /// This patches the account's balance directly via the sandbox RPC,
    /// useful for testing scenarios that require specific balances
    /// (e.g., staking tests that need 1M+ NEAR).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit::{NearToken, account};
    /// use near_kit_sandbox::Sandbox;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = Sandbox::builder().shared().await?;
    ///
    /// // Set account balance to 1M NEAR for staking test
    /// sandbox.set_balance(account!("validator.sandbox"), NearToken::from_near(1_000_000)).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_balance(
        &self,
        account_id: impl Into<AccountId>,
        balance: NearToken,
    ) -> Result<(), SandboxError> {
        let near = self.near();
        let account_id = account_id.into();

        // Fetch raw account data from RPC - this includes all fields the sandbox expects
        let mut account_response: serde_json::Value = near
            .rpc()
            .request(
                "query",
                serde_json::json!({
                    "finality": "optimistic",
                    "request_type": "view_account",
                    "account_id": account_id.to_string()
                }),
            )
            .await?;

        account_response["amount"] = balance.as_yoctonear().to_string().into();

        let records = serde_json::json!([
            {
                "Account": {
                    "account_id": account_id.to_string(),
                    "account": account_response
                }
            }
        ]);

        self.patch_state(records).await?;

        // The patch applies asynchronously and is racy. Rather than trust a
        // fixed delay, poll until the new balance is observable so callers can
        // read it back immediately. Bounded so a genuinely failed patch still
        // returns.
        const OBSERVATION_ATTEMPTS: u32 = 50;
        let mut last_error = None;
        for _ in 0..OBSERVATION_ATTEMPTS {
            match near
                .rpc()
                .view_account(&account_id, near_kit::rpc::Finality::Final)
                .await
            {
                Ok(account) if account.amount == balance => return Ok(()),
                Ok(_) => {}
                Err(error) => last_error = Some(Box::new(error.into())),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        Err(SandboxError::PatchTimeout {
            account_id,
            attempts: OBSERVATION_ATTEMPTS,
            last_error,
        })
    }

    /// Write raw state records (accounts, access keys, contract code or data)
    /// straight into the sandbox with `sandbox_patch_state`.
    ///
    /// `records` is the JSON array the RPC method takes. The patch is sent
    /// twice, because patching account state sometimes only takes effect
    /// reliably on the second send (see
    /// <https://github.com/near/near-workspaces-rs/commit/2b72b9b8491c3140ff2d30b0c45d09b200cb027b>).
    /// It applies asynchronously: poll for the state you wrote before relying
    /// on it, as [`set_balance`](Self::set_balance) does.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit_sandbox::Sandbox;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = Sandbox::builder().shared().await?;
    /// sandbox
    ///     .patch_state(serde_json::json!([
    ///         { "Data": { "account_id": "counter.sandbox", "data_key": "U1RBVEU=", "value": "AQAAAA==" } }
    ///     ]))
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn patch_state(&self, records: serde_json::Value) -> Result<(), SandboxError> {
        let near = self.near();
        for _ in 0..2 {
            let _: serde_json::Value = near
                .rpc()
                .request(
                    "sandbox_patch_state",
                    serde_json::json!({ "records": records }),
                )
                .await?;
        }
        Ok(())
    }

    /// Fast-forward the sandbox by `delta_height` blocks.
    ///
    /// Useful for testing time-dependent logic (e.g., lockups, staking epoch
    /// changes) without waiting for real block production.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use near_kit_sandbox::Sandbox;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = Sandbox::builder().shared().await?;
    /// sandbox.fast_forward(1000).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn fast_forward(&self, delta_height: u64) -> Result<(), SandboxError> {
        // Sent once: `RpcClient::request` never retries this method, because a
        // retry after a lost response could advance the chain twice.
        let _: serde_json::Value = self
            .near()
            .rpc()
            .request(
                "sandbox_fast_forward",
                serde_json::json!({ "delta_height": delta_height }),
            )
            .await?;
        Ok(())
    }
}

// ============================================================================
// SandboxBuilder
// ============================================================================

/// Builder for custom sandbox configuration.
///
/// Created via [`Sandbox::builder()`].
pub struct SandboxBuilder {
    version: Option<String>,
    root_account: Option<AccountId>,
    chain_id: Option<ChainId>,
    construction_error: Option<SandboxError>,
}

impl SandboxBuilder {
    async fn start(self) -> Result<Sandbox, SandboxError> {
        if let Some(error) = self.construction_error {
            return Err(error);
        }
        let version = self.version.unwrap_or_else(|| DEFAULT_VERSION.to_owned());
        let root_account = self
            .root_account
            .unwrap_or_else(|| SANDBOX_ROOT_ACCOUNT.into());
        let chain_id = self.chain_id;
        info!(
            version = %version,
            root_account = %root_account,
            chain_id = chain_id.as_ref().map(ChainId::as_str).unwrap_or("(default)"),
            "Starting sandbox container"
        );

        let root_key: SecretKey = SANDBOX_ROOT_SECRET_KEY
            .parse()
            .expect("SANDBOX_ROOT_SECRET_KEY is a valid ed25519 key");
        let root_signer = Arc::new(InMemorySigner::new(&root_account, root_key));
        let image = NearSandbox::new(&version, &root_account, chain_id.as_ref());
        let container_err = |operation| {
            move |error: TestcontainersError| SandboxError::Container {
                operation,
                message: error.to_string(),
            }
        };
        let container = image
            .with_health_check(Healthcheck::empty().with_start_period(HEALTHCHECK_START_PERIOD))
            .with_startup_timeout(STARTUP_TIMEOUT)
            .start()
            .await
            .map_err(container_err("start"))?;
        let container_id = container.id().to_string();
        let host = container
            .get_host()
            .await
            .map_err(container_err("host discovery"))?;
        let host_port = container
            .get_host_port_ipv4(RPC_PORT)
            .await
            .map_err(container_err("RPC port discovery"))?;
        let rpc_url = format!("http://{host}:{host_port}");

        info!(rpc_url = %rpc_url, "Sandbox container ready");

        Ok(Sandbox {
            _container: Arc::new(container),
            container_id,
            rpc_url,
            chain_id: chain_id.unwrap_or_else(|| ChainId::new("sandbox")),
            root_signer,
        })
    }

    /// Set the sandbox version to use.
    ///
    /// If not specified, uses the bundled default nearcore version.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit_sandbox::Sandbox;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = Sandbox::builder()
    ///     .version("2.13.4")
    ///     .fresh()
    ///     .await?;
    /// # Ok(())
    /// # }
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
    /// ```rust,no_run
    /// # use near_kit::account;
    /// # use near_kit_sandbox::Sandbox;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = Sandbox::builder()
    ///     .root_account(account!("sb"))
    ///     .fresh()
    ///     .await?;
    /// // Sub-accounts will be "alice.sb" instead of "alice.sandbox"
    /// # Ok(())
    /// # }
    /// ```
    pub fn root_account(mut self, name: impl Into<AccountId>) -> Self {
        self.root_account = Some(name.into());
        self
    }

    /// Set the chain ID for this sandbox.
    ///
    /// Useful for testing chain-ID-dependent logic (e.g., signed wallet
    /// requests that compare against the chain ID).
    ///
    /// If not specified, the sandbox uses its default chain ID. Sandboxes
    /// that share a chain ID still keep their nonces apart: near-kit caches
    /// nonces by each chain's genesis hash, and every sandbox has its own.
    ///
    /// **Note:** `"mainnet"` and `"testnet"` are not supported. Attempting to
    /// use either is reported by [`SandboxBuilder::fresh`] or
    /// [`SandboxBuilder::shared`].
    /// `neard` reserves these chain IDs for the real networks and rejects the
    /// sandbox test-seed during `init`, so the container never passes its
    /// health check (surfacing only as an opaque container-startup failure).
    /// Use a custom chain ID instead (e.g. `"localnet"`, or `"pinet"` for
    /// Private Shard).
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use near_kit_sandbox::Sandbox;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let sandbox = Sandbox::builder()
    ///     .chain_id("localnet")
    ///     .fresh()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn chain_id(mut self, chain_id: impl Into<String>) -> Self {
        let chain_id = chain_id.into();
        if matches!(chain_id.as_str(), "mainnet" | "testnet") {
            self.construction_error
                .get_or_insert(SandboxError::UnsupportedChainId(chain_id));
        } else {
            self.chain_id = Some(ChainId::new(chain_id));
        }
        self
    }

    /// Spawn a fresh sandbox with the configured options.
    ///
    /// The sandbox container will be stopped when the last clone of
    /// the returned [`Sandbox`] is dropped.
    pub async fn fresh(self) -> Result<Sandbox, SandboxError> {
        self.start().await
    }

    /// Get or create the shared sandbox with the configured options.
    ///
    /// **Note:** The version, root account, and chain ID are only used if the
    /// shared sandbox hasn't been initialized yet. If it's already running, the
    /// existing instance is returned regardless of the options specified here.
    pub async fn shared(self) -> Result<&'static Sandbox, SandboxError> {
        SHARED_SANDBOX
            .get_or_try_init(|| async move {
                let sandbox = self.start().await?;
                // SAFETY: `cleanup_shared_sandbox` is a plain `extern "C" fn`
                // that never unwinds across the C boundary.
                unsafe { libc::atexit(cleanup_shared_sandbox) };
                Ok(sandbox)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_secret_key_parses() {
        SANDBOX_ROOT_SECRET_KEY.parse::<SecretKey>().unwrap();
    }

    #[test]
    fn chain_id_mainnet_is_deferred_as_an_error() {
        let builder = Sandbox::builder().chain_id("mainnet");
        assert!(matches!(
            builder.construction_error,
            Some(SandboxError::UnsupportedChainId(chain_id)) if chain_id == "mainnet"
        ));
    }

    #[test]
    fn chain_id_custom_is_accepted() {
        let builder = Sandbox::builder().chain_id("localnet");
        assert!(builder.construction_error.is_none());
        assert_eq!(
            builder.chain_id.as_ref().map(ChainId::as_str),
            Some("localnet")
        );
    }

    #[test]
    fn builder_preserves_the_first_construction_error() {
        let builder = Sandbox::builder().chain_id("mainnet").chain_id("testnet");
        assert!(matches!(
            builder.construction_error,
            Some(SandboxError::UnsupportedChainId(chain_id)) if chain_id == "mainnet"
        ));
    }
}
