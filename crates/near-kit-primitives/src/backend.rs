//! The two chain verbs ([`Viewer`], [`TxSender`]) and the awaitable values
//! that generated contract clients return ([`View`], [`Call`]).
//!
//! Nothing here names an RPC client: `near-kit`'s `Near` implements these
//! traits, and so can any other backend (a gateway, a test mock, a sender
//! that relays through someone else's account).

use std::fmt;
use std::future::{Future, IntoFuture};
use std::sync::Arc;

use crate::error::CodecError;
use crate::platform::{BoxFuture, MaybeSend, MaybeSync};
use crate::transaction::FunctionCall;
use crate::types::{AccountId, Action, BlockReference, Gas, NearToken, ViewFunctionResult};

/// A backend that can run contract view functions.
///
/// This is the read verb: one raw `call_function` query, returning the bytes
/// the contract produced and the block they were read at. Generated contract
/// clients decode those bytes; implementers only move them.
///
/// Implement it with an `async fn`. The future must be `Send` on every target
/// except `wasm32-unknown-unknown`.
///
/// ```rust
/// use near_kit_primitives::rpc::{BlockReference, ViewFunctionResult};
/// use near_kit_primitives::{AccountId, CryptoHash, Viewer};
///
/// /// Answers every view with `42`.
/// struct Fixed;
///
/// impl Viewer for Fixed {
///     type ViewError = std::convert::Infallible;
///
///     async fn view_function(
///         &self,
///         _contract_id: &AccountId,
///         _method: &str,
///         _args: &[u8],
///         _block: BlockReference,
///     ) -> Result<ViewFunctionResult, Self::ViewError> {
///         Ok(ViewFunctionResult::new(b"42".to_vec(), 1, CryptoHash::ZERO))
///     }
/// }
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot run contract views",
    label = "not a `Viewer`",
    note = "use a `Near` client, or implement `Viewer` for this backend"
)]
pub trait Viewer: MaybeSend + MaybeSync {
    /// The backend's error. Unbounded, so `anyhow::Error` or a string works.
    type ViewError;

    /// Run `method` on `contract_id` with already-encoded `args`, reading the
    /// state at `block`.
    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, Self::ViewError>> + MaybeSend;
}

/// A backend that can send transactions from a fixed account.
///
/// This is the write verb: a transaction is a receiver plus actions, and the
/// backend decides how to sign, how long to wait and what to return
/// ([`Output`](Self::Output)).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot send transactions",
    label = "needs a signer",
    note = "`near.with_signer(signer)` returns a sender; or implement `TxSender`"
)]
pub trait TxSender: MaybeSend + MaybeSync {
    /// What a sent transaction resolves to, such as an execution outcome or
    /// just a transaction hash.
    type Output;
    /// The backend's error. Unbounded.
    type SendError;

    /// The account transactions are sent from.
    fn account_id(&self) -> &AccountId;

    /// Send one transaction with `actions` to `receiver_id`.
    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<Self::Output, Self::SendError>> + MaybeSend;
}

/// A typed contract interface: ties the marker type that `#[contract]`
/// generates to its client.
///
/// For `#[contract] trait Counter`, the macro implements this as
/// `type Client<C> = CounterClient<C>`. `near_kit::Near::contract::<Counter>(id)`
/// calls [`client`](Self::client) with a clone of the client; any other
/// backend can call it directly:
///
/// ```rust,ignore
/// use near_kit_primitives::Contract;
///
/// let counter = Counter::client(my_backend, account_id);
/// let count = counter.get_count().await?;
/// ```
pub trait Contract {
    /// The generated client, generic over its backend.
    type Client<C>;

    /// A client for the contract at `contract_id` that talks through
    /// `backend`.
    fn client<C>(backend: C, contract_id: AccountId) -> Self::Client<C>;
}

/// Error from a [`View`] or [`Call`].
///
/// Displays as the underlying error. It is a [`std::error::Error`] whenever
/// the backend error is `Debug + Display`, not only when it is itself an
/// `Error`, so `ContractError<anyhow::Error>` converts into `anyhow::Error`
/// with `?`. [`source`](std::error::Error::source) is the codec error's
/// source; a backend error's own source chain isn't reachable through it.
#[derive(Debug)]
#[non_exhaustive]
pub enum ContractError<E> {
    /// The arguments failed to encode, or the view result failed to decode.
    Codec(CodecError),
    /// The backend failed.
    Backend(E),
}

impl<E: fmt::Display> fmt::Display for ContractError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => error.fmt(f),
            Self::Backend(error) => error.fmt(f),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for ContractError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec(error) => std::error::Error::source(error),
            Self::Backend(_) => None,
        }
    }
}

/// A typed contract view, ready to `.await`.
///
/// Returned by the view methods of a `#[contract]` client and by
/// `near_kit::Near::view`; build one for any [`Viewer`] with
/// [`json`](Self::json) or [`borsh`](Self::borsh), which pick how the result
/// is decoded. Reads at [`BlockReference::default()`] (final) unless
/// [`at`](Self::at) says otherwise. Awaiting it yields the decoded value;
/// use [`with_metadata`](Self::with_metadata) to also get the block it was
/// read at.
///
/// ```rust
/// use near_kit_primitives::transaction::View;
/// use near_kit_primitives::rpc::{BlockReference, ViewFunctionResult};
/// use near_kit_primitives::{AccountId, CryptoHash, Viewer, account};
///
/// struct Fixed;
///
/// impl Viewer for Fixed {
///     type ViewError = std::convert::Infallible;
///
///     async fn view_function(
///         &self,
///         _contract_id: &AccountId,
///         _method: &str,
///         _args: &[u8],
///         _block: BlockReference,
///     ) -> Result<ViewFunctionResult, Self::ViewError> {
///         Ok(ViewFunctionResult::new(b"42".to_vec(), 1, CryptoHash::ZERO))
///     }
/// }
///
/// # futures::executor::block_on(async {
/// let count: u64 = View::json(Fixed, account!("counter.near"), "get_count")
///     .args(serde_json::json!({ "at": "now" }))
///     .at(1_000_u64)
///     .await?;
/// assert_eq!(count, 42);
/// # Ok::<(), near_kit_primitives::transaction::ContractError<std::convert::Infallible>>(())
/// # }).unwrap();
/// ```
///
/// A `View` owns its backend (generated clients clone theirs into it), so a
/// helper can return one and its future is `'static` whenever the backend
/// is: it can be spawned. To share a backend that isn't `Clone`, hand the
/// client an `Arc` of it. A borrowed backend (`&B`) works too, but then
/// `.await` needs a `'static` borrow; [`with_metadata`](Self::with_metadata)
/// has no such bound.
#[must_use = "a view does nothing until it is awaited"]
pub struct View<C, T> {
    backend: C,
    contract_id: AccountId,
    method: String,
    // Arguments are encoded when the view is created, so borrowed argument
    // types never end up inside the future.
    args: Result<Vec<u8>, CodecError>,
    block: BlockReference,
    decode: fn(&[u8]) -> Result<T, CodecError>,
}

impl<C, T> View<C, T> {
    fn new(
        backend: C,
        contract_id: AccountId,
        method: String,
        decode: fn(&[u8]) -> Result<T, CodecError>,
    ) -> Self {
        Self {
            backend,
            contract_id,
            method,
            args: Ok(Vec::new()),
            block: BlockReference::default(),
            decode,
        }
    }

    /// A view of `method` on `contract_id` whose result is decoded as JSON.
    ///
    /// It sends no arguments until [`args`](Self::args),
    /// [`args_borsh`](Self::args_borsh) or [`args_raw`](Self::args_raw) sets
    /// them. An empty result (a void method) decodes as `null`, so `()` and
    /// `Option<_>` work.
    pub fn json(backend: C, contract_id: impl Into<AccountId>, method: impl Into<String>) -> Self
    where
        C: Viewer,
        T: serde::de::DeserializeOwned,
    {
        Self::new(backend, contract_id.into(), method.into(), |bytes| {
            let bytes = if bytes.is_empty() { b"null" } else { bytes };
            Ok(serde_json::from_slice(bytes)?)
        })
    }

    /// A view of `method` on `contract_id` whose result is decoded as Borsh.
    ///
    /// Like [`json`](Self::json), it sends no arguments until one of the
    /// `args` methods sets them.
    pub fn borsh(backend: C, contract_id: impl Into<AccountId>, method: impl Into<String>) -> Self
    where
        C: Viewer,
        T: borsh::BorshDeserialize,
    {
        Self::new(backend, contract_id.into(), method.into(), |bytes| {
            borsh::from_slice(bytes).map_err(CodecError::Borsh)
        })
    }

    /// JSON-encode `args` as the view's arguments.
    ///
    /// An encoding error is kept and returned from `.await`. Each `args`
    /// method replaces what an earlier one set, error included.
    pub fn args(mut self, args: impl serde::Serialize) -> Self {
        self.args = serde_json::to_vec(&args).map_err(CodecError::from);
        self
    }

    /// Borsh-encode `args` as the view's arguments. See [`args`](Self::args).
    pub fn args_borsh(mut self, args: impl borsh::BorshSerialize) -> Self {
        self.args = borsh::to_vec(&args).map_err(CodecError::Borsh);
        self
    }

    /// Send `args` as the view's arguments as they are.
    pub fn args_raw(mut self, args: impl Into<Vec<u8>>) -> Self {
        self.args = Ok(args.into());
        self
    }

    /// Read at a specific block height, hash or finality.
    pub fn at(mut self, block: impl Into<BlockReference>) -> Self {
        self.block = block.into();
        self
    }

    /// Run the view and keep the block height, block hash and logs alongside
    /// the decoded value.
    pub async fn with_metadata(self) -> Result<ViewFunctionResult<T>, ContractError<C::ViewError>>
    where
        C: Viewer,
    {
        let args = match self.args {
            Ok(args) => args,
            Err(error) => return Err(ContractError::Codec(error)),
        };
        // `match` rather than `?` on the backend result: a `?` temporary
        // holding `C::ViewError` would make the future `!Send` for a `!Send`
        // error.
        let raw = match self
            .backend
            .view_function(&self.contract_id, &self.method, &args, self.block)
            .await
        {
            Ok(raw) => raw,
            Err(error) => return Err(ContractError::Backend(error)),
        };
        let result = (self.decode)(&raw.result).map_err(ContractError::Codec)?;
        Ok(ViewFunctionResult::new(result, raw.block_height, raw.block_hash).with_logs(raw.logs))
    }
}

impl<C, T> fmt::Debug for View<C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("View")
            .field("contract_id", &self.contract_id)
            .field("method", &self.method)
            .field("args", &self.args.as_ref().map(Vec::len))
            .field("block", &self.block)
            .finish_non_exhaustive()
    }
}

impl<C, T> IntoFuture for View<C, T>
where
    C: Viewer + 'static,
    C::ViewError: 'static,
    T: 'static,
{
    type Output = Result<T, ContractError<C::ViewError>>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.with_metadata().await.map(|view| view.result) })
    }
}

/// A typed contract call, ready to `.await`.
///
/// Returned by the `#[call]` methods of a `#[contract]` client. Awaiting it
/// sends one transaction with this function call through the client's
/// [`TxSender`] and resolves to the sender's [`Output`](TxSender::Output).
///
/// Like [`View`], it owns its backend, so its future is `'static` whenever
/// the backend is.
#[must_use = "a call does nothing until it is awaited"]
pub struct Call<C> {
    backend: C,
    contract_id: AccountId,
    call: FunctionCall,
}

impl<C> Call<C> {
    pub(crate) fn new(backend: C, contract_id: AccountId, call: FunctionCall) -> Self {
        Self {
            backend,
            contract_id,
            call,
        }
    }

    /// Set the gas limit (default 30 Tgas).
    pub fn gas(mut self, gas: Gas) -> Self {
        self.call = self.call.gas(gas);
        self
    }

    /// Attach a deposit (default zero).
    pub fn deposit(mut self, deposit: NearToken) -> Self {
        self.call = self.call.deposit(deposit);
        self
    }
}

impl<C> fmt::Debug for Call<C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("contract_id", &self.contract_id)
            .field("call", &self.call)
            .finish_non_exhaustive()
    }
}

impl<C> IntoFuture for Call<C>
where
    C: TxSender + 'static,
    C::Output: 'static,
    C::SendError: 'static,
{
    type Output = Result<C::Output, ContractError<C::SendError>>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let Self {
            backend,
            contract_id,
            call,
        } = self;
        let action = Action::try_from(call);
        Box::pin(async move {
            let action = match action {
                Ok(action) => action,
                Err(error) => return Err(ContractError::Codec(error)),
            };
            // Fully qualified, so an inherent `send` on the backend can't
            // shadow the trait method.
            match TxSender::send(&backend, contract_id, vec![action]).await {
                Ok(output) => Ok(output),
                Err(error) => Err(ContractError::Backend(error)),
            }
        })
    }
}

/// Forwards to `T`, so a borrowed backend can serve several clients.
impl<T: Viewer + ?Sized> Viewer for &T {
    type ViewError = T::ViewError;

    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, Self::ViewError>> + MaybeSend {
        (**self).view_function(contract_id, method, args, block)
    }
}

/// Forwards to `T`.
impl<T: Viewer + ?Sized> Viewer for Box<T> {
    type ViewError = T::ViewError;

    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, Self::ViewError>> + MaybeSend {
        (**self).view_function(contract_id, method, args, block)
    }
}

/// Forwards to `T`, so a backend that isn't `Clone` can serve several
/// clients and still give `'static` futures.
impl<T: Viewer + ?Sized> Viewer for Arc<T> {
    type ViewError = T::ViewError;

    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, Self::ViewError>> + MaybeSend {
        (**self).view_function(contract_id, method, args, block)
    }
}

/// Forwards to `T`, so a borrowed sender can serve several clients.
impl<T: TxSender + ?Sized> TxSender for &T {
    type Output = T::Output;
    type SendError = T::SendError;

    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<Self::Output, Self::SendError>> + MaybeSend {
        (**self).send(receiver_id, actions)
    }
}

/// Forwards to `T`.
impl<T: TxSender + ?Sized> TxSender for Box<T> {
    type Output = T::Output;
    type SendError = T::SendError;

    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<Self::Output, Self::SendError>> + MaybeSend {
        (**self).send(receiver_id, actions)
    }
}

/// Forwards to `T`, so a sender that isn't `Clone` can serve several
/// clients and still give `'static` futures.
impl<T: TxSender + ?Sized> TxSender for Arc<T> {
    type Output = T::Output;
    type SendError = T::SendError;

    fn account_id(&self) -> &AccountId {
        (**self).account_id()
    }

    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<Self::Output, Self::SendError>> + MaybeSend {
        (**self).send(receiver_id, actions)
    }
}

/// Constructors and encoders used by `#[contract]` expansions (and by
/// near-kit's token clients). Not public API.
pub(crate) mod expansion {
    use super::*;

    /// A JSON-decoded view.
    pub fn view_json<C, T>(
        backend: C,
        contract_id: AccountId,
        method: &'static str,
        args: Result<Vec<u8>, CodecError>,
    ) -> View<C, T>
    where
        C: Viewer,
        T: serde::de::DeserializeOwned,
    {
        View {
            args,
            ..View::json(backend, contract_id, method)
        }
    }

    /// A Borsh-decoded view.
    pub fn view_borsh<C, T>(
        backend: C,
        contract_id: AccountId,
        method: &'static str,
        args: Result<Vec<u8>, CodecError>,
    ) -> View<C, T>
    where
        C: Viewer,
        T: borsh::BorshDeserialize,
    {
        View {
            args,
            ..View::borsh(backend, contract_id, method)
        }
    }

    /// A call through `backend`.
    pub fn call<C>(backend: C, contract_id: AccountId, call: FunctionCall) -> Call<C>
    where
        C: TxSender,
    {
        Call::new(backend, contract_id, call)
    }

    /// JSON-encode view arguments.
    pub fn json_args<A: serde::Serialize + ?Sized>(args: &A) -> Result<Vec<u8>, CodecError> {
        Ok(serde_json::to_vec(args)?)
    }

    /// Borsh-encode view arguments.
    pub fn borsh_args<A: borsh::BorshSerialize + ?Sized>(args: &A) -> Result<Vec<u8>, CodecError> {
        borsh::to_vec(args).map_err(CodecError::Borsh)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::expansion::{borsh_args, call, json_args, view_borsh, view_json};
    use super::*;
    use crate::types::{CryptoHash, Finality, FunctionCallAction};

    /// A view request as the backend saw it.
    type Seen = (AccountId, String, Vec<u8>, BlockReference);

    /// Records the last view request and answers with fixed bytes.
    struct MockViewer {
        answer: Vec<u8>,
        seen: Mutex<Option<Seen>>,
    }

    impl MockViewer {
        /// Shared, so a test can hand a clone to the view and still read
        /// what the backend saw.
        fn new(answer: &[u8]) -> Arc<Self> {
            Arc::new(Self {
                answer: answer.to_vec(),
                seen: Mutex::new(None),
            })
        }
    }

    impl Viewer for MockViewer {
        type ViewError = String;

        async fn view_function(
            &self,
            contract_id: &AccountId,
            method: &str,
            args: &[u8],
            block: BlockReference,
        ) -> Result<ViewFunctionResult, String> {
            *self.seen.lock().unwrap() =
                Some((contract_id.clone(), method.to_owned(), args.to_vec(), block));
            if method == "fail" {
                return Err("backend down".to_owned());
            }
            Ok(
                ViewFunctionResult::new(self.answer.clone(), 7, CryptoHash::ZERO)
                    .with_logs(vec!["log".to_owned()]),
            )
        }
    }

    /// Records sent transactions and returns how many actions it got.
    #[derive(Default)]
    struct MockSender {
        sent: Mutex<Vec<(AccountId, Vec<Action>)>>,
    }

    impl TxSender for MockSender {
        type Output = usize;
        type SendError = String;

        fn account_id(&self) -> &AccountId {
            unreachable!("not used by Call")
        }

        async fn send(
            &self,
            receiver_id: AccountId,
            actions: Vec<Action>,
        ) -> Result<usize, String> {
            let count = actions.len();
            self.sent.lock().unwrap().push((receiver_id, actions));
            Ok(count)
        }
    }

    struct FailingJson;

    impl serde::Serialize for FailingJson {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("deliberate"))
        }
    }

    fn run<F: IntoFuture>(future: F) -> F::Output {
        futures::executor::block_on(future.into_future())
    }

    fn contract() -> AccountId {
        "counter.near".parse().unwrap()
    }

    #[test]
    fn json_view_decodes_and_defaults_to_final() {
        let viewer = MockViewer::new(b"42");
        let value: u64 = run(view_json(
            viewer.clone(),
            contract(),
            "get",
            json_args(&()).map(|_| b"{}".to_vec()),
        ))
        .unwrap();
        assert_eq!(value, 42);

        let (id, method, args, block) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(id, contract());
        assert_eq!(method, "get");
        assert_eq!(args, b"{}");
        assert_eq!(block, BlockReference::default());
    }

    #[test]
    fn json_view_reads_empty_result_as_null() {
        let viewer = MockViewer::new(b"");
        let value: Option<u64> =
            run(view_json(viewer.clone(), contract(), "get", Ok(Vec::new()))).unwrap();
        assert_eq!(value, None);
        run(view_json::<_, ()>(
            viewer.clone(),
            contract(),
            "get",
            Ok(Vec::new()),
        ))
        .unwrap();
    }

    #[test]
    fn borsh_view_decodes() {
        let viewer = MockViewer::new(&borsh::to_vec(&7_u32).unwrap());
        let args = borsh_args(&1_u8);
        let value: u32 = run(view_borsh(viewer.clone(), contract(), "get", args)).unwrap();
        assert_eq!(value, 7);
        let (_, _, args, _) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(args, [1]);
    }

    #[test]
    fn at_and_with_metadata() {
        let viewer = MockViewer::new(b"\"hi\"");
        let result = run(
            view_json::<_, String>(viewer.clone(), contract(), "get", Ok(Vec::new()))
                .at(Finality::Optimistic)
                .with_metadata(),
        )
        .unwrap();
        assert_eq!(result.result, "hi");
        assert_eq!(result.block_height, 7);
        assert_eq!(result.logs, ["log"]);
        let (_, _, _, block) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(block, BlockReference::Finality(Finality::Optimistic));
    }

    #[test]
    fn view_errors_keep_their_layer() {
        let viewer = MockViewer::new(b"bad");

        let error = run(view_json::<_, u64>(
            viewer.clone(),
            contract(),
            "get",
            json_args(&FailingJson),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));
        assert!(viewer.seen.lock().unwrap().is_none(), "nothing was sent");

        let error = run(view_json::<_, u64>(
            viewer.clone(),
            contract(),
            "fail",
            Ok(Vec::new()),
        ))
        .unwrap_err();
        assert!(matches!(&error, ContractError::Backend(e) if e == "backend down"));

        let error = run(view_json::<_, u64>(
            viewer.clone(),
            contract(),
            "get",
            Ok(Vec::new()),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));

        let error = run(view_borsh::<_, u64>(
            viewer.clone(),
            contract(),
            "get",
            Ok(Vec::new()),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Borsh(_))));
    }

    struct FailingBorsh;

    impl borsh::BorshSerialize for FailingBorsh {
        fn serialize<W: std::io::Write>(&self, _: &mut W) -> std::io::Result<()> {
            Err(std::io::Error::other("deliberate"))
        }
    }

    #[test]
    fn public_constructors_send_no_args_until_set() {
        let viewer = MockViewer::new(b"");
        let value: Option<u64> =
            run(View::json(viewer.clone(), contract(), String::from("get"))).unwrap();
        assert_eq!(value, None);
        let (_, method, args, _) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!((method.as_str(), args), ("get", Vec::new()));

        let viewer = MockViewer::new(&borsh::to_vec(&9_u16).unwrap());
        let value: u16 =
            run(View::borsh(viewer.clone(), contract(), "get").args_borsh(3_u8)).unwrap();
        assert_eq!(value, 9);
        let (_, _, args, _) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(args, [3]);
    }

    #[test]
    fn argument_encode_errors_are_returned_from_await() {
        let viewer = MockViewer::new(b"1");
        let error = run(View::<_, u64>::json(viewer.clone(), contract(), "get").args(FailingJson))
            .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));
        let error =
            run(View::<_, u64>::borsh(viewer.clone(), contract(), "get").args_borsh(FailingBorsh))
                .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Borsh(_))));
        assert!(viewer.seen.lock().unwrap().is_none(), "nothing was sent");
    }

    #[test]
    fn last_argument_setter_wins() {
        type MockView = View<Arc<MockViewer>, u64>;
        let viewer = MockViewer::new(b"1");
        let view =
            |set: fn(MockView) -> MockView| set(View::json(viewer.clone(), contract(), "get")).args;

        assert_eq!(
            view(|v| v.args(FailingJson).args_raw([1, 2, 3])).unwrap(),
            [1, 2, 3]
        );
        assert_eq!(
            view(|v| v
                .args_borsh(FailingBorsh)
                .args(serde_json::json!({ "valid": true })))
            .unwrap(),
            br#"{"valid":true}"#
        );
        assert!(matches!(
            view(|v| v.args_raw(vec![1]).args(FailingJson)),
            Err(CodecError::Json(_))
        ));
        assert!(matches!(
            view(|v| v.args_raw(vec![1]).args_borsh(FailingBorsh)),
            Err(CodecError::Borsh(_))
        ));
    }

    #[test]
    fn call_sends_one_function_call() {
        let sender = Arc::new(MockSender::default());
        let count = run(
            call(sender.clone(), contract(), FunctionCall::new("add").args(5))
                .gas(Gas::from_tgas(50))
                .deposit(NearToken::from_yoctonear(1)),
        )
        .unwrap();
        assert_eq!(count, 1);

        let (receiver, actions) = sender.sent.lock().unwrap().pop().unwrap();
        assert_eq!(receiver, contract());
        assert_eq!(
            actions,
            [Action::FunctionCall(FunctionCallAction {
                method_name: "add".to_owned(),
                args: b"5".to_vec(),
                gas: Gas::from_tgas(50),
                deposit: NearToken::from_yoctonear(1),
            })]
        );
    }

    #[test]
    fn call_encode_error_sends_nothing() {
        let sender = Arc::new(MockSender::default());
        let error = run(call(
            sender.clone(),
            contract(),
            FunctionCall::new("add").args(FailingJson),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));
        assert!(sender.sent.lock().unwrap().is_empty());
    }

    #[test]
    fn contract_error_is_an_error_for_display_only_backend_errors() {
        /// Like `anyhow::Error`: `Debug + Display`, but not an `Error`.
        #[derive(Debug)]
        struct DisplayOnly;

        impl fmt::Display for DisplayOnly {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("display only")
            }
        }

        fn into_boxed(
            result: Result<(), ContractError<DisplayOnly>>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            result?;
            Ok(())
        }

        let error = into_boxed(Err(ContractError::Backend(DisplayOnly))).unwrap_err();
        assert_eq!(error.to_string(), "display only");
    }

    #[test]
    fn contract_error_is_a_std_error() {
        let error: ContractError<std::io::Error> =
            ContractError::Backend(std::io::Error::other("down"));
        let error: &dyn std::error::Error = &error;
        assert_eq!(error.to_string(), "down");
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn futures_are_send_and_static() {
        fn assert_spawnable<F: Future + Send + 'static>(_: F) {}
        fn generic<V, S>(viewer: V, sender: S)
        where
            V: Viewer + 'static,
            S: TxSender + 'static,
            V::ViewError: Send,
            S::Output: Send,
            S::SendError: Send,
        {
            assert_spawnable(
                view_json::<_, u64>(viewer, contract(), "get", Ok(Vec::new())).into_future(),
            );
            assert_spawnable(call(sender, contract(), FunctionCall::new("add")).into_future());
        }
        generic(MockViewer::new(b"1"), Arc::new(MockSender::default()));
        generic(
            Box::new(MockViewer::new(b"1")),
            Box::new(MockSender::default()),
        );
    }

    #[test]
    fn borrowed_and_boxed_backends_forward() {
        let viewer = MockViewer::new(b"5");
        let by_ref: u64 = futures::executor::block_on(
            view_json::<_, u64>(&*viewer, contract(), "get", Ok(Vec::new())).with_metadata(),
        )
        .unwrap()
        .result;
        assert_eq!(by_ref, 5);

        let boxed: Box<MockSender> = Box::default();
        assert_eq!(
            run(call(boxed, contract(), FunctionCall::new("add"))).unwrap(),
            1
        );

        let sender = MockSender::default();
        let borrowed = &sender;
        let sent = futures::executor::block_on(TxSender::send(
            &borrowed,
            contract(),
            vec![Action::transfer(NearToken::from_yoctonear(1))],
        ))
        .unwrap();
        assert_eq!(sent, 1);
        assert_eq!(sender.sent.lock().unwrap().len(), 1);
    }
}
