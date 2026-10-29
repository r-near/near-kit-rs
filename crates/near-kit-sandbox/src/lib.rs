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
//! `root.near().with_signer(signer)`, which reuses the connection. For code
//! configured with an RPC URL and chain ID (such as
//! `SigningNear::from_env`, through `NEAR_NETWORK` and `NEAR_CHAIN_ID`), the
//! sandbox's are `root.near().rpc().url()` and `root.near().chain_id()`.
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
//! For a sandbox node this crate didn't start (one a test launched itself and
//! reaches by URL), the free functions [`patch_state`] and [`fast_forward`]
//! do the same through a `near_kit::rpc::RpcClient`.
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

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use near_kit::AccountIdRef;
use near_kit::protocol::ChainId;
use near_kit::rpc::{ReqwestTransport, RpcClient, RpcError};
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
#[non_exhaustive]
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

/// The sandbox image (inlined from near-sandbox-testcontainer), configured
/// through its entrypoint's `NEAR_*` environment variables.
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

/// Global shared sandbox instance.
static SHARED_SANDBOX: OnceCell<Sandbox> = OnceCell::const_new();

/// Remove the shared sandbox container on normal process exit (statics are
/// never dropped; the testcontainers watchdog covers signals).
///
/// No tokio here: by the time `atexit` fires, the main thread's TLS is gone
/// and the async Docker client would panic, aborting the process. So this
/// shells out to `docker rm`, inside `catch_unwind`, and ignores errors.
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

/// A sandbox instance backed by a Docker container via testcontainers.
///
/// Cloning a `Sandbox` shares the underlying container — the container is stopped
/// only when the last `Arc` reference is dropped.
///
/// Obtain one via `Sandbox::builder().shared()` (global singleton) or
/// `Sandbox::builder().fresh()` (new isolated instance); see
/// [`Sandbox::builder`]. The shared sandbox is removed when the process exits.
#[derive(Clone)]
pub struct Sandbox {
    _container: Arc<ContainerAsync<NearSandbox>>,
    /// Docker container id, captured at start so the `atexit` handler can remove
    /// the shared container synchronously without touching the async client.
    container_id: String,
    /// The read-only client for this sandbox's RPC endpoint, built once at
    /// start and shared by every method and every `root_account` client.
    near: Near,
    root_signer: Arc<dyn Signer>,
}

impl Sandbox {
    /// A builder for a sandbox, finished with [`fresh`](SandboxBuilder::fresh)
    /// or [`shared`](SandboxBuilder::shared).
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
        self.near.with_signer(self.root_signer.clone())
    }

    /// Set an account's balance in this sandbox, patching it directly via the
    /// sandbox RPC.
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
        let near = &self.near;
        let account_id = account_id.into();

        // Fetch raw account data from RPC - this includes all fields the sandbox expects
        let account_response: serde_json::Value = near
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

        let records = serde_json::json!([
            {
                "Account": {
                    "account_id": account_id.to_string(),
                    "account": with_amount(account_response, balance)
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
    /// `records` is the JSON array the RPC method takes. This is
    /// [`patch_state`] on this sandbox's RPC client, so the
    /// patch is sent twice and applies asynchronously: poll for the state you
    /// wrote before relying on it, as [`set_balance`](Self::set_balance) does.
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
        crate::patch_state(self.near.rpc(), records).await
    }

    /// Fast-forward the sandbox by `delta_height` blocks, without waiting for
    /// real block production.
    ///
    /// This is [`fast_forward`] on this sandbox's RPC
    /// client.
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
        crate::fast_forward(self.near.rpc(), delta_height).await
    }
}

/// Write raw state records (accounts, access keys, contract code or data)
/// into a sandbox node with `sandbox_patch_state`.
///
/// Use this for a sandbox node this crate didn't start, such as one a test
/// launched itself and reaches by URL; on a [`Sandbox`], call
/// [`Sandbox::patch_state`]. `records` is the JSON array the RPC method takes.
///
/// The patch is sent twice, because patching account state sometimes only
/// takes effect reliably on the second send (see
/// <https://github.com/near/near-workspaces-rs/commit/2b72b9b8491c3140ff2d30b0c45d09b200cb027b>).
/// It applies asynchronously: poll for the state you wrote before relying on
/// it.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::rpc::RpcClient;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let rpc = RpcClient::new("http://127.0.0.1:3030");
/// near_kit_sandbox::patch_state(
///     &rpc,
///     serde_json::json!([
///         { "Data": { "account_id": "counter.sandbox", "data_key": "U1RBVEU=", "value": "AQAAAA==" } }
///     ]),
/// )
/// .await?;
/// # Ok(())
/// # }
/// ```
pub async fn patch_state(rpc: &RpcClient, records: serde_json::Value) -> Result<(), SandboxError> {
    let params = serde_json::json!({ "records": records });
    for _ in 0..2 {
        let _: serde_json::Value = rpc.request("sandbox_patch_state", &params).await?;
    }
    Ok(())
}

/// Fast-forward a sandbox node by `delta_height` blocks with
/// `sandbox_fast_forward`, without waiting for real block production.
///
/// Use this for a sandbox node this crate didn't start, such as one a test
/// launched itself and reaches by URL; on a [`Sandbox`], call
/// [`Sandbox::fast_forward`].
///
/// The request is sent once: [`RpcClient::request`] never retries this
/// method, because a retry after a lost response could advance the chain
/// twice.
///
/// # Example
///
/// ```rust,no_run
/// use near_kit::rpc::RpcClient;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let rpc = RpcClient::new("http://127.0.0.1:3030");
/// near_kit_sandbox::fast_forward(&rpc, 1000).await?;
/// # Ok(())
/// # }
/// ```
pub async fn fast_forward(rpc: &RpcClient, delta_height: u64) -> Result<(), SandboxError> {
    let _: serde_json::Value = rpc
        .request(
            "sandbox_fast_forward",
            serde_json::json!({ "delta_height": delta_height }),
        )
        .await?;
    Ok(())
}

/// `account` (a raw `view_account` result) with its `amount` set to `balance`.
///
/// A result that isn't a JSON object is passed through unchanged, so the
/// patch or the balance check reports the failure instead of a panic.
fn with_amount(mut account: serde_json::Value, balance: NearToken) -> serde_json::Value {
    if let Some(fields) = account.as_object_mut() {
        fields.insert(
            "amount".to_string(),
            serde_json::Value::String(balance.as_yoctonear().to_string()),
        );
    }
    account
}

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

        // No idle connections are kept, so each request opens its connection
        // on the calling tokio runtime. A pooled connection is driven by a task
        // on the runtime that opened it, and the shared sandbox serves tests
        // that each run their own runtime: a request reusing another test's
        // connection would fail once that test ends and its runtime drops.
        let client = near_kit::reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .expect("the HTTP client's TLS backend initializes");
        let near = Near::custom(rpc_url, chain_id.unwrap_or_else(|| ChainId::new("sandbox")))
            .transport(ReqwestTransport::with_client(client))
            .build();

        Ok(Sandbox {
            _container: Arc::new(container),
            container_id,
            near,
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
    pub fn root_account(mut self, name: impl Into<AccountId>) -> Self {
        self.root_account = Some(name.into());
        self
    }

    /// Set the chain ID for this sandbox; without one, the image default is used.
    ///
    /// Sandboxes that share a chain ID still keep their nonces apart: near-kit
    /// caches nonces by each chain's genesis hash, and every sandbox has its
    /// own. `"mainnet"` and `"testnet"` are reported as
    /// [`SandboxError::UnsupportedChainId`] by `fresh` or `shared`.
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
    /// The version, root account, and chain ID are only used if the shared
    /// sandbox hasn't been initialized yet. If it's already running, the
    /// existing instance is returned regardless of the options specified here.
    /// An invalid option (see [`chain_id`](Self::chain_id)) is reported either
    /// way.
    pub async fn shared(mut self) -> Result<&'static Sandbox, SandboxError> {
        if let Some(error) = self.construction_error.take() {
            return Err(error);
        }
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

    /// Both entry points report the deferred error before touching Docker.
    #[tokio::test]
    async fn fresh_and_shared_report_the_construction_error() {
        let fresh = Sandbox::builder().chain_id("testnet").fresh().await;
        assert!(matches!(
            fresh,
            Err(SandboxError::UnsupportedChainId(chain_id)) if chain_id == "testnet"
        ));
        let shared = Sandbox::builder().chain_id("mainnet").shared().await;
        assert!(matches!(
            shared,
            Err(SandboxError::UnsupportedChainId(chain_id)) if chain_id == "mainnet"
        ));
    }

    #[test]
    fn with_amount_sets_the_amount_of_an_account_object_only() {
        let balance = NearToken::from_near(5);
        let account = serde_json::json!({ "amount": "1", "locked": "0" });
        assert_eq!(
            with_amount(account, balance),
            serde_json::json!({ "amount": balance.as_yoctonear().to_string(), "locked": "0" })
        );
        for other in [
            serde_json::json!(null),
            serde_json::json!([1, 2]),
            serde_json::json!("account"),
            serde_json::json!(7),
        ] {
            assert_eq!(with_amount(other.clone(), balance), other);
        }
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

    /// Records every JSON-RPC request and answers each with `{}`, as the
    /// sandbox methods do.
    #[derive(Default)]
    struct Recorder(std::sync::Mutex<Vec<serde_json::Value>>);

    impl near_kit::rpc::RpcTransport for Recorder {
        fn post_json(
            &self,
            _url: &str,
            body: Vec<u8>,
        ) -> near_kit::rpc::BoxFuture<'_, Result<near_kit::rpc::TransportResponse, RpcError>>
        {
            self.0
                .lock()
                .unwrap()
                .push(serde_json::from_slice(&body).unwrap());
            let body = br#"{"jsonrpc":"2.0","id":0,"result":{}}"#.to_vec();
            Box::pin(async move { Ok(near_kit::rpc::TransportResponse { status: 200, body }) })
        }
    }

    fn recording_rpc() -> (RpcClient, Arc<Recorder>) {
        let recorder = Arc::new(Recorder::default());
        let rpc = RpcClient::with_transport_and_retry_config(
            "http://127.0.0.1:3030",
            recorder.clone(),
            near_kit::rpc::RetryConfig::none(),
        );
        (rpc, recorder)
    }

    #[tokio::test]
    async fn patch_state_sends_the_records_twice() {
        let (rpc, recorder) = recording_rpc();
        let records = serde_json::json!([
            { "Data": { "account_id": "counter.sandbox", "data_key": "U1RBVEU=", "value": "AQAAAA==" } }
        ]);
        patch_state(&rpc, records.clone()).await.unwrap();

        let requests = recorder.0.lock().unwrap();
        assert_eq!(requests.len(), 2);
        for request in requests.iter() {
            assert_eq!(request["method"], "sandbox_patch_state");
            assert_eq!(request["params"], serde_json::json!({ "records": records }));
        }
    }

    #[tokio::test]
    async fn fast_forward_sends_delta_height_once() {
        let (rpc, recorder) = recording_rpc();
        fast_forward(&rpc, 205).await.unwrap();

        let requests = recorder.0.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "sandbox_fast_forward");
        assert_eq!(
            requests[0]["params"],
            serde_json::json!({ "delta_height": 205 })
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
