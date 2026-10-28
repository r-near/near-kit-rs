# Testing

Read this when you write tests for code that uses near-kit. There are three
levels, from fastest to most faithful:

- **Mock backends.** Typed contract clients, `View`, `Call` and the token
  clients run on any [`Viewer`](crate::Viewer) or
  [`TxSender`](crate::TxSender). A test backend that answers views from a
  table and records sends needs no network and no async runtime beyond the
  one the test already has.
- **Canned RPC responses.** A [`RpcTransport`](crate::rpc::RpcTransport)
  that answers JSON-RPC requests from the test exercises a real `Near`,
  including its response parsing and error mapping.
- **The sandbox.** The `near-kit-sandbox` crate starts a local NEAR node in
  Docker, so transactions really execute, contracts really run, and nonces,
  gas and receipts behave as they do on a network.

The first two run anywhere `cargo test` runs. The sandbox needs Docker and
takes seconds to start, so most projects keep it for a smaller set of
integration tests.

## Write code against the traits

Code is easy to mock when it takes a contract client or a sender as a
parameter, generic over its backend, instead of building a `Near` itself.
Production passes a client over `Near` or `SigningNear`; tests pass the same
client over a mock:

```rust
# use serde::{Deserialize, Serialize};
# #[near_kit::contract]
# pub trait Guestbook {
#     fn get_messages(&self, args: GetMessages) -> Vec<Message>;
#     fn total_messages(&self) -> u64;
#     #[call]
#     fn add_message(&mut self, args: AddMessage);
# }
# #[derive(Serialize)]
# pub struct GetMessages { pub from_index: Option<u64>, pub limit: Option<u64> }
# #[derive(Serialize)]
# pub struct AddMessage { pub text: String }
# #[derive(Debug, Deserialize)]
# pub struct Message { pub premium: bool, pub sender: String, pub text: String }
use near_kit::*;

/// How many of the latest 100 messages are premium.
async fn premium_count<C>(guestbook: &GuestbookClient<C>) -> Result<usize, ContractError<C::ViewError>>
where
    C: Viewer + Clone + 'static,
    C::ViewError: 'static,
{
    let messages = guestbook
        .get_messages(GetMessages { from_index: None, limit: Some(100) })
        .await?;
    Ok(messages.iter().filter(|m| m.premium).count())
}

// In production:
# async fn example(near: &Near) -> Result<(), ContractError<near_kit::rpc::RpcError>> {
let guestbook = near.contract::<Guestbook>(account!("guestbook.testnet"));
let count = premium_count(&guestbook).await?;
# Ok(())
# }
```

The bounds come from the generated client: view methods need
`C: Viewer + Clone`, and awaiting the returned `View` needs the backend and
its error to be `'static`, because the future owns a clone of the backend.
`#[call]` methods need `C: TxSender + Clone` in the same way.

Code that sends transactions without a contract client can still be
generic: [`TxSender::send`](crate::TxSender::send) takes a receiver and a
list of actions, and on a `SigningNear` it is the same as
`alice.transaction(receiver)` with those actions, including nonce retries:

```rust
use near_kit::protocol::Action;
use near_kit::*;

/// Pays a fixed fee to the treasury.
async fn pay_fee<S: TxSender>(sender: &S) -> Result<S::Output, S::SendError> {
    sender
        .send(account!("treasury.testnet"), vec![Action::transfer(NearToken::from_millinear(100))])
        .await
}
```

Code that calls `alice.transaction(..)` or `alice.sign(..)` directly takes a
`SigningNear` and is tested with canned RPC responses or the sandbox.

## Mock views

A [`Viewer`](crate::Viewer) gets the contract, the method name, the encoded
arguments and the block, and returns the bytes the contract would have
returned, with the block they were read at. This one answers from a table of
JSON values and records each call. It is `Clone`, so it can back a
generated client, and the clones share the record:

```rust
# use serde::{Deserialize, Serialize};
# #[near_kit::contract]
# pub trait Guestbook {
#     fn get_messages(&self, args: GetMessages) -> Vec<Message>;
#     fn total_messages(&self) -> u64;
#     #[call]
#     fn add_message(&mut self, args: AddMessage);
# }
# #[derive(Serialize)]
# pub struct GetMessages { pub from_index: Option<u64>, pub limit: Option<u64> }
# #[derive(Serialize)]
# pub struct AddMessage { pub text: String }
# #[derive(Debug, Deserialize)]
# pub struct Message { pub premium: bool, pub sender: String, pub text: String }
# async fn premium_count<C>(guestbook: &GuestbookClient<C>) -> Result<usize, ContractError<C::ViewError>>
# where C: Viewer + Clone + 'static, C::ViewError: 'static {
#     let messages = guestbook.get_messages(GetMessages { from_index: None, limit: Some(100) }).await?;
#     Ok(messages.iter().filter(|m| m.premium).count())
# }
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::*;
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct MockViewer {
    replies: Arc<HashMap<&'static str, Value>>,
    calls: Arc<Mutex<Vec<(String, Value)>>>,
}

impl MockViewer {
    fn new(replies: impl IntoIterator<Item = (&'static str, Value)>) -> Self {
        Self { replies: Arc::new(replies.into_iter().collect()), ..Default::default() }
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().unwrap().clone()
    }
}

impl Viewer for MockViewer {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        method: &str,
        args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        let args = serde_json::from_slice(args).unwrap_or(Value::Null);
        self.calls.lock().unwrap().push((method.to_string(), args));
        let reply = self.replies.get(method).ok_or(format!("no reply for {method}"))?;
        Ok(ViewFunctionResult::new(reply.to_string().into_bytes(), 1, CryptoHash::ZERO))
    }
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
// In a #[tokio::test]:
let mock = MockViewer::new([(
    "get_messages",
    json!([
        { "premium": true, "sender": "alice.testnet", "text": "hi" },
        { "premium": false, "sender": "bob.testnet", "text": "hello" },
    ]),
)]);
let guestbook = Guestbook::client(mock.clone(), account!("guestbook.testnet"));

assert_eq!(premium_count(&guestbook).await?, 1);
assert_eq!(
    mock.calls(),
    [("get_messages".to_string(), json!({ "from_index": null, "limit": 100 }))],
);

// A method without a reply fails as the backend's error.
let error = guestbook.total_messages().await.unwrap_err();
assert!(matches!(error, ContractError::Backend(message) if message.contains("total_messages")));
# Ok(())
# }
```

[`Contract::client`](crate::Contract::client) builds a generated client
over any backend; `near.contract::<Guestbook>(id)` is the same call with a
clone of the `Near`. A JSON method without arguments sends `{}`, and a view
whose reply doesn't decode fails with `ContractError::Codec`, so tests can
cover both.

[`View::json`](crate::View::json) works on the same mock for code that
calls views by name rather than through a typed client:

```rust
# use std::sync::Arc;
# use near_kit::rpc::{BlockReference, ViewFunctionResult};
# use near_kit::*;
# #[derive(Clone)]
# struct MockViewer;
# impl Viewer for MockViewer {
#     type ViewError = String;
#     async fn view_function(&self, _: &AccountId, _: &str, _: &[u8], _: BlockReference) -> Result<ViewFunctionResult, String> {
#         Ok(ViewFunctionResult::new(b"3".to_vec(), 1, CryptoHash::ZERO))
#     }
# }
# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let total: u64 = View::json(MockViewer, account!("guestbook.testnet"), "total_messages").await?;
assert_eq!(total, 3);
# Ok(())
# }
```

## Record sends

A [`TxSender`](crate::TxSender) picks its own
[`Output`](crate::TxSender::Output). A recorder can return `()` and keep
the receiver and actions of each transaction for the test to inspect:

```rust
# use serde::{Deserialize, Serialize};
# #[near_kit::contract]
# pub trait Guestbook {
#     fn get_messages(&self, args: GetMessages) -> Vec<Message>;
#     fn total_messages(&self) -> u64;
#     #[call]
#     fn add_message(&mut self, args: AddMessage);
# }
# #[derive(Serialize)]
# pub struct GetMessages { pub from_index: Option<u64>, pub limit: Option<u64> }
# #[derive(Serialize)]
# pub struct AddMessage { pub text: String }
# #[derive(Debug, Deserialize)]
# pub struct Message { pub premium: bool, pub sender: String, pub text: String }
use std::sync::{Arc, Mutex};

use near_kit::protocol::Action;
use near_kit::standards::FungibleToken;
use near_kit::*;
use serde_json::json;

#[derive(Clone)]
struct Recorder {
    account_id: AccountId,
    sent: Arc<Mutex<Vec<(AccountId, Vec<Action>)>>>,
}

impl Recorder {
    fn new(account_id: AccountId) -> Self {
        Self { account_id, sent: Default::default() }
    }

    fn sent(&self) -> Vec<(AccountId, Vec<Action>)> {
        self.sent.lock().unwrap().clone()
    }
}

impl TxSender for Recorder {
    type Output = ();
    type SendError = std::convert::Infallible;

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    async fn send(&self, receiver_id: AccountId, actions: Vec<Action>) -> Result<(), Self::SendError> {
        self.sent.lock().unwrap().push((receiver_id, actions));
        Ok(())
    }
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let alice = Recorder::new(account!("alice.testnet"));

FungibleToken::client(alice.clone(), account!("usdc.testnet"))
    .transfer(account!("bob.testnet"), 1_500_000_u128, None)
    .await?;
Guestbook::client(alice.clone(), account!("guestbook.testnet"))
    .add_message(AddMessage { text: "hi".into() })
    .deposit(NearToken::from_millinear(10))
    .await?;

let sent = alice.sent();
assert_eq!(sent.len(), 2);

let (receiver, actions) = &sent[0];
assert_eq!(receiver, "usdc.testnet");
let [Action::FunctionCall(call)] = actions.as_slice() else { panic!("one function call") };
assert_eq!(call.method_name, "ft_transfer");
assert_eq!(call.deposit, NearToken::from_yoctonear(1));
assert_eq!(call.gas, Gas::from_tgas(30));
let args: serde_json::Value = serde_json::from_slice(&call.args)?;
assert_eq!(args, json!({ "receiver_id": "bob.testnet", "amount": "1500000" }));

let [Action::FunctionCall(call)] = sent[1].1.as_slice() else { panic!("one function call") };
assert_eq!(call.method_name, "add_message");
assert_eq!(call.deposit, NearToken::from_millinear(10));
# Ok(())
# }
```

The assertions check exactly what would reach the chain: the method name,
the encoded arguments, the gas and the deposit. That's where most mistakes
in calling code are, such as a missing 1 yoctoNEAR deposit or an amount
sent as a number instead of a string.

To test failure handling, return an error from `send`. The caller sees it as
`ContractError::Backend(e)` from a typed call, or `e` from
`TxSender::send`.

## Shared, boxed and borrowed backends

Both traits are implemented for `&T`, `Box<T>` and `Arc<T>` whenever `T`
implements them, including unsized `T`. Two consequences:

- A backend that isn't `Clone` can back a generated client through
  `Arc<T>`, or through `&T` for views read with
  [`View::with_metadata`](crate::View::with_metadata). Awaiting a `View` or
  `Call` directly needs a `'static` backend, so a borrowed one only works
  there if it is `&'static`.
- `Viewer` and `TxSender` have `async` methods, so `dyn TxSender` isn't a
  type. To choose a backend at run time, put a small object-safe trait in
  front of it and implement `TxSender` for that trait object. `Arc<dyn _>`
  is then a `Clone` sender that every client accepts:

```rust
use std::future::Future;
use std::sync::Arc;

use near_kit::protocol::Action;
use near_kit::rpc::{BoxFuture, FinalExecutionOutcome};
use near_kit::standards::FungibleToken;
use near_kit::*;

/// Any sender that waits for `Final` and fails with `near_kit::Error`.
trait Wallet: MaybeSend + MaybeSync {
    fn account(&self) -> &AccountId;
    fn send_boxed(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> BoxFuture<'_, Result<FinalExecutionOutcome, Error>>;
}

impl<S> Wallet for S
where
    S: TxSender<Output = FinalExecutionOutcome, SendError = Error>,
{
    fn account(&self) -> &AccountId {
        self.account_id()
    }

    fn send_boxed(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> BoxFuture<'_, Result<FinalExecutionOutcome, Error>> {
        Box::pin(self.send(receiver_id, actions))
    }
}

impl TxSender for dyn Wallet {
    type Output = FinalExecutionOutcome;
    type SendError = Error;

    fn account_id(&self) -> &AccountId {
        self.account()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<FinalExecutionOutcome, Error>> + MaybeSend {
        self.send_boxed(receiver_id, actions)
    }
}

# fn example(alice: SigningNear) {
let wallet: Arc<dyn Wallet> = Arc::new(alice);
let token = FungibleToken::client(wallet, account!("usdc.testnet"));
# }
```

`MaybeSend` and `MaybeSync` are `Send` and `Sync` on every target except
`wasm32-unknown-unknown`, where they are empty. Use them instead of
`Send + Sync` in bounds on code that should also build for the browser.

## Canned RPC responses

A mock backend skips near-kit's RPC layer. To test code that takes a `Near`
or `SigningNear`, or to test how your code handles a particular RPC error,
give a real client a transport that answers from the test.
[`RpcTransport`](crate::rpc::RpcTransport) has one method: it gets the
JSON-RPC request body and returns an HTTP status and a response body.

```rust
use std::sync::{Arc, Mutex};

use near_kit::rpc::{BoxFuture, RetryConfig, RpcError, RpcTransport, TransportResponse};
use near_kit::*;
use serde_json::{Value, json};

type Handler = dyn Fn(&str, &Value) -> Value + Send + Sync;

/// Answers each request with `{"result": ..}` or `{"error": ..}` from a closure.
struct Canned {
    handler: Box<Handler>,
    requests: Mutex<Vec<Value>>,
}

impl RpcTransport for Canned {
    fn post_json(&self, _url: &str, body: Vec<u8>) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        let request: Value = serde_json::from_slice(&body).unwrap();
        let mut reply = (self.handler)(request["method"].as_str().unwrap(), &request["params"]);
        reply["jsonrpc"] = json!("2.0");
        reply["id"] = request["id"].clone();
        self.requests.lock().unwrap().push(request);
        let body = serde_json::to_vec(&reply).unwrap();
        Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
    }
}

# #[tokio::main(flavor = "current_thread")]
# async fn main() -> Result<(), Box<dyn std::error::Error>> {
let transport = Arc::new(Canned {
    handler: Box::new(|method, params| match (method, params["account_id"].as_str()) {
        ("EXPERIMENTAL_view_account", Some("alice.testnet")) => json!({ "result": {
            "amount": NearToken::from_near(5),
            "locked": NearToken::ZERO,
            "code_hash": CryptoHash::ZERO,
            "storage_usage": 182,
            "block_height": 100,
            "block_hash": CryptoHash::ZERO,
        }}),
        ("EXPERIMENTAL_view_account", Some(other)) => json!({ "error": {
            "code": -32000,
            "message": "Server error",
            "name": "HANDLER_ERROR",
            "cause": { "name": "UNKNOWN_ACCOUNT", "info": { "requested_account_id": other } },
        }}),
        ("EXPERIMENTAL_call_function", _) => json!({ "result": {
            "result": b"3".to_vec(),
            "logs": [],
            "block_height": 100,
            "block_hash": CryptoHash::ZERO,
        }}),
        _ => json!({ "error": { "code": -32601, "message": format!("unexpected {method}") } }),
    }),
    requests: Mutex::default(),
});

let near = Near::custom("http://mock", "mocknet")
    .transport(transport.clone())
    .retry_config(RetryConfig::none())
    .build();

// Parsing: the balance is computed from the account view.
let balance = near.balance(account!("alice.testnet")).await?;
assert_eq!(balance.total, NearToken::from_near(5));

// Error mapping: UNKNOWN_ACCOUNT becomes a typed error, and `false` here.
assert!(!near.account_exists(account!("ghost.testnet")).await?);
let error = near.rpc().view_account(&account!("ghost.testnet"), near_kit::rpc::Finality::Final).await.unwrap_err();
assert!(error.is_account_not_found());

// Views go through `EXPERIMENTAL_call_function`, with base64 arguments.
let total: u64 = near.view(account!("guestbook.testnet"), "total_messages").await?;
assert_eq!(total, 3);
let requests = transport.requests.lock().unwrap();
assert_eq!(requests.last().unwrap()["params"]["method_name"], "total_messages");
assert_eq!(requests.last().unwrap()["params"]["finality"], "final");
# Ok(())
# }
```

Some details that matter when writing replies:

- **Which method.** near-kit calls `EXPERIMENTAL_view_account` for account
  reads, `EXPERIMENTAL_view_access_key` for a single key and
  `EXPERIMENTAL_call_function` for views; reads such as code, state and key
  lists use `query` with a `request_type`. Record the requests, as above,
  to see what a code path sends.
- **Errors.** A reply with an `error` object whose `cause.name` nearcore
  uses (`UNKNOWN_ACCOUNT`, `UNKNOWN_ACCESS_KEY`, `INVALID_TRANSACTION`, ...)
  becomes the matching typed [`RpcError`](crate::rpc::RpcError), as it
  would from a node. The [errors chapter](crate::guide::errors) lists the
  variants.
- **Retries.** Turn them off with
  [`RetryConfig::none`](crate::rpc::RetryConfig::none). Otherwise a reply
  the client treats as transient, such as an HTTP 503 or an error with
  code -32603, is re-sent with backoff, and the test waits. To test the
  retries themselves, keep a retry config with short delays and count the
  requests.
- **Nonces.** A `SigningNear` over a canned transport uses the
  process-wide nonce cache like any other. Give each test its own chain ID
  in `Near::custom`, or its own account or key, so tests running in
  parallel don't share nonce sequences.

A send through a canned transport needs replies for
`EXPERIMENTAL_view_access_key` (the nonce and a recent block hash) and
`send_tx`. For testing what is sent, a recording
`TxSender` is less work. For testing how a send behaves on chain, use the
sandbox.

## The sandbox

`near-kit-sandbox` runs `nearprotocol/sandbox` in Docker through
testcontainers. Add it as a development dependency:

```sh
cargo add --dev near-kit-sandbox
cargo add --dev tokio --features macros,rt-multi-thread
```

Docker must be running. If Docker listens on a non-default socket, set
`DOCKER_HOST` to it before running the tests.

```rust,ignore
// `ignore`: near-kit-sandbox depends on near-kit, so near-kit's own
// doctests can't use it. The same code compiles in a crate that has both.
use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::*;
use near_kit_sandbox::Sandbox;

#[tokio::test]
async fn alice_can_send() -> Result<(), Box<dyn std::error::Error>> {
    let sandbox = Sandbox::builder().shared().await?;
    let root = sandbox.root_account();

    // The root account is funded at genesis; create a test account under it.
    let alice_id: AccountId = format!("alice-{}.{}", std::process::id(), root.account_id()).parse()?;
    let alice_key = SecretKey::generate_ed25519();
    root.transaction(&alice_id)
        .add_action(Action::create_account())
        .add_action(Action::transfer(NearToken::from_near(10)))
        .add_action(Action::add_full_access_key(alice_key.public_key()))
        .await?
        .result()?;

    let alice = root.near().with_signer(InMemorySigner::new(&alice_id, alice_key));
    let outcome = alice
        .transaction(root.account_id())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await?;
    assert!(outcome.is_success());
    Ok(())
}
```

[`sandbox.root_account()`](https://docs.rs/near-kit-sandbox/latest/near_kit_sandbox/struct.Sandbox.html#method.root_account)
returns a `SigningNear` for the root account, `sandbox` unless the builder
names another. Accounts it creates are sub-accounts of it. Reads go through
`root.near()`, and signing as another account is
`root.near().with_signer(..)`.

### Shared or fresh

- `Sandbox::builder().shared()` starts one sandbox for the whole test
  process on first use and returns the same `&'static Sandbox` afterwards.
  Startup is paid once, but state is shared: give each test its own
  accounts, as the example does with the process ID in the name.
- `Sandbox::builder().fresh()` starts a new sandbox with clean state, which
  is stopped when the last clone of the returned `Sandbox` drops. Use it for
  tests that need a known starting state or change global things.

The builder also takes `.version("..")` for a nearcore version,
`.root_account(account!(".."))` and `.chain_id("..")`. Settings only apply
when the shared sandbox is first started. `mainnet` and `testnet` aren't
accepted as chain IDs, because nearcore reserves them. `NEAR_SANDBOX_IMAGE`
overrides the Docker image and tag, for a mirror or a custom build.

### Changing state directly

Sandbox-only RPC methods change chain state without transactions:

```rust,ignore
// `ignore`: needs near-kit-sandbox, which near-kit's doctests can't use.
use near_kit::*;
use near_kit_sandbox::Sandbox;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let sandbox = Sandbox::builder().fresh().await?;

// Waits until the new balance is visible to reads.
sandbox.set_balance(account!("sandbox"), NearToken::from_near(1_000_000)).await?;

// Raw state records, as `sandbox_patch_state` takes them. Returns once the
// node accepts the patch; poll before relying on the new state.
sandbox
    .patch_state(serde_json::json!([
        { "Data": { "account_id": "counter.sandbox", "data_key": "U1RBVEU=", "value": "AQAAAA==" } }
    ]))
    .await?;

// Produce 1000 blocks at once, for lockups, expiry and epoch changes.
sandbox.fast_forward(1_000).await?;
# Ok(())
# }
```

`set_balance` polls until the balance changes. `patch_state` sends the
patch twice, because account patches sometimes only take effect on the
second send, but it doesn't poll; read the state you wrote in a loop
before the test depends on it. Data keys and values in a patch are base64
of the raw bytes the contract stores.

The `near-kit-sandbox` [API documentation](https://docs.rs/near-kit-sandbox)
covers the rest.

## See also

- [`Viewer`](crate::Viewer), [`TxSender`](crate::TxSender),
  [`Contract`](crate::Contract), [`View`](crate::View),
  [`Call`](crate::Call), [`ContractError`](crate::ContractError)
- [`RpcTransport`](crate::rpc::RpcTransport),
  [`TransportResponse`](crate::rpc::TransportResponse),
  [`RetryConfig`](crate::rpc::RetryConfig)
- The [contracts chapter](crate::guide::contracts_and_tokens) for the
  `#[contract]` macro
