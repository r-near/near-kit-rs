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
/// Generated clients need a `Clone` backend; `&T` and `Arc<T>` forward to
/// `T`. See [`View`] for which backends `.await` accepts and for an example
/// implementation.
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
///
/// Generated clients need a `Clone` backend; `&T` and `Arc<T>` forward to
/// `T`. Through a short borrow, send with [`Call::send`].
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
#[cfg_attr(feature = "contracts", doc = "```rust")]
#[cfg_attr(not(feature = "contracts"), doc = "```rust,ignore")]
/// use near_kit_primitives::{AccountId, Contract, ContractError, Viewer};
///
/// #[near_kit_primitives::contract]
/// pub trait Counter {
///     fn get_count(&self) -> u64;
/// }
///
/// # async fn count<V>(backend: V, contract_id: AccountId) -> Result<u64, ContractError<V::ViewError>>
/// # where
/// #     V: Viewer + Clone + 'static,
/// #     V::ViewError: 'static,
/// # {
/// // `backend` is any `Viewer` (and `TxSender`, for `#[call]` methods).
/// let counter = Counter::client(backend, contract_id);
/// let count = counter.get_count().await?;
/// # Ok(count)
/// # }
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
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ContractError<E> {
    /// The arguments failed to encode, or the view result failed to decode.
    #[error(transparent)]
    Codec(CodecError),
    /// The backend failed.
    #[error("{0}")]
    Backend(E),
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
/// use near_kit_primitives::View;
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
/// # Ok::<(), near_kit_primitives::ContractError<std::convert::Infallible>>(())
/// # }).unwrap();
/// ```
///
/// A `View` owns its backend (generated clients clone theirs into it), so a
/// helper can return one and its future is `'static` whenever the backend
/// is: it can be spawned. To share a backend that isn't `Clone`, hand the
/// client an `Arc` of it. A borrowed backend (`&B`) works too, but then
/// `.await` needs a `'static` borrow; [`with_metadata`](Self::with_metadata)
/// has no such bound, and its `.result` is the value `.await` would give.
///
/// A `View` is [`Clone`] when its backend is, so one view can be run again
/// and again. [`fetch_raw`](Self::fetch_raw) runs it without decoding and
/// [`decode`](Self::decode) decodes later, for instance only when a polled
/// result changed.
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
        args: Vec<u8>,
        decode: fn(&[u8]) -> Result<T, CodecError>,
    ) -> Self {
        Self {
            backend,
            contract_id,
            method,
            args: Ok(args),
            block: BlockReference::default(),
            decode,
        }
    }

    /// A view of `method` on `contract_id` whose result is decoded as JSON.
    ///
    /// It sends the empty JSON object `{}` until [`args`](Self::args),
    /// [`args_borsh`](Self::args_borsh) or [`args_raw`](Self::args_raw) sets
    /// other arguments, as a `#[contract]` view without arguments does. A
    /// method whose parameters are all `Option` needs that object; empty
    /// bytes fail to parse. An empty result (a void method) decodes as
    /// `null`, so `()` and `Option<_>` work.
    pub fn json(backend: C, contract_id: impl Into<AccountId>, method: impl Into<String>) -> Self
    where
        C: Viewer,
        T: serde::de::DeserializeOwned,
    {
        Self::new(
            backend,
            contract_id.into(),
            method.into(),
            b"{}".to_vec(),
            |bytes| {
                let bytes = if bytes.is_empty() { b"null" } else { bytes };
                Ok(serde_json::from_slice(bytes)?)
            },
        )
    }

    /// A view of `method` on `contract_id` whose result is decoded as Borsh.
    ///
    /// It sends no arguments (empty bytes, the Borsh encoding of `()`) until
    /// one of the `args` methods sets them.
    pub fn borsh(backend: C, contract_id: impl Into<AccountId>, method: impl Into<String>) -> Self
    where
        C: Viewer,
        T: borsh::BorshDeserialize,
    {
        Self::new(
            backend,
            contract_id.into(),
            method.into(),
            Vec::new(),
            |bytes| borsh::from_slice(bytes).map_err(CodecError::Borsh),
        )
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
        let decode = self.decode;
        let raw = self.fetch_raw().await?;
        let result = decode(&raw.result).map_err(ContractError::Codec)?;
        Ok(ViewFunctionResult::new(result, raw.block_height, raw.block_hash).with_logs(raw.logs))
    }

    /// Run the view without decoding its result: the bytes the contract
    /// returned, with the block they were read at and the logs.
    ///
    /// [`decode`](Self::decode) turns those bytes into what `.await` would
    /// return. Splitting the two lets a caller look at the bytes first, for
    /// example to skip decoding when a polled view returns the same bytes as
    /// last time:
    ///
    /// ```rust
    /// # use near_kit_primitives::{ContractError, View, Viewer};
    /// # async fn poll<C: Viewer + Clone>(view: View<C, u64>, last: &mut Vec<u8>)
    /// #     -> Result<Option<u64>, ContractError<C::ViewError>> {
    /// let raw = view.clone().fetch_raw().await?;
    /// if raw.result == *last {
    ///     return Ok(None);
    /// }
    /// let value = view.decode(&raw.result).map_err(ContractError::Codec)?;
    /// *last = raw.result;
    /// Ok(Some(value))
    /// # }
    /// ```
    ///
    /// An argument encoding error is returned as
    /// [`ContractError::Codec`] without calling the backend.
    pub async fn fetch_raw(self) -> Result<ViewFunctionResult, ContractError<C::ViewError>>
    where
        C: Viewer,
    {
        let args = self.args.map_err(ContractError::Codec)?;
        // `match` rather than `?` on the backend result: a `?` temporary
        // holding `C::ViewError` would make the future `!Send` for a `!Send`
        // error.
        match self
            .backend
            .view_function(&self.contract_id, &self.method, &args, self.block)
            .await
        {
            Ok(raw) => Ok(raw),
            Err(error) => Err(ContractError::Backend(error)),
        }
    }

    /// Decode a result as this view would: JSON or Borsh, as chosen by
    /// [`json`](Self::json) or [`borsh`](Self::borsh) (for JSON, empty bytes
    /// read as `null`). Pair it with [`fetch_raw`](Self::fetch_raw).
    pub fn decode(&self, bytes: &[u8]) -> Result<T, CodecError> {
        (self.decode)(bytes)
    }
}

/// Clones the backend, the encoded arguments and the block, so one view can
/// be run again, for instance on every tick of a poll.
///
/// A view whose arguments failed to encode clones into one that fails the
/// same way: same [`CodecError`] variant and message (and Borsh
/// [`io::ErrorKind`](std::io::ErrorKind)), without the original's source
/// chain.
impl<C: Clone, T> Clone for View<C, T> {
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            contract_id: self.contract_id.clone(),
            method: self.method.clone(),
            args: match &self.args {
                Ok(args) => Ok(args.clone()),
                Err(error) => Err(copy_codec_error(error)),
            },
            block: self.block,
            decode: self.decode,
        }
    }
}

/// `CodecError` holds `serde_json::Error` and `io::Error`, neither of which is
/// `Clone`; rebuild one with the same variant and message.
fn copy_codec_error(error: &CodecError) -> CodecError {
    match error {
        CodecError::Json(error) => CodecError::Json(serde::ser::Error::custom(error)),
        CodecError::Borsh(error) => {
            CodecError::Borsh(std::io::Error::new(error.kind(), error.to_string()))
        }
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
/// the backend is: `.await` boxes it, which needs an owned, `Arc` or
/// `'static` backend. With a shorter borrow (a local `&sender`), use
/// [`send`](Self::send)`.await`, which has no such bound.
#[must_use = "a call does nothing until it is awaited"]
pub struct Call<C> {
    backend: C,
    contract_id: AccountId,
    call: FunctionCall,
}

impl<C> Call<C> {
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

    /// Send the call: what `.await` does, without boxing a `'static`
    /// future, so it also works through a borrowed backend (`&sender`).
    pub async fn send(self) -> Result<C::Output, ContractError<C::SendError>>
    where
        C: TxSender,
    {
        let action = Action::try_from(self.call).map_err(ContractError::Codec)?;
        // Fully qualified, so an inherent `send` on the backend can't shadow
        // the trait method.
        TxSender::send(&self.backend, self.contract_id, vec![action])
            .await
            .map_err(ContractError::Backend)
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

/// The function call a [`Call`] would send, without its backend or receiver.
///
/// Keeps the method, arguments, gas and deposit, so a typed call can be
/// batched with other actions in one transaction to the same contract, or
/// embedded in another contract's promise. An argument encoding error
/// carries over and surfaces when the `FunctionCall` becomes an action.
///
/// ```rust
/// # use near_kit_primitives::Gas;
/// # use near_kit_primitives::transaction::FunctionCall;
/// # use near_kit_primitives::Call;
/// # fn batch<C>(call: Call<C>) -> Result<(), near_kit_primitives::transaction::CodecError> {
/// let action = FunctionCall::from(call.gas(Gas::from_tgas(50))).into_action()?;
/// # Ok(())
/// # }
/// ```
impl<C> From<Call<C>> for FunctionCall {
    fn from(call: Call<C>) -> Self {
        call.call
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
        Box::pin(self.send())
    }
}

macro_rules! forward {
    ($($p:ty),+) => {$(
        /// Forwards to `T`.
        impl<T: Viewer + ?Sized> Viewer for $p {
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
        impl<T: TxSender + ?Sized> TxSender for $p {
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
    )+};
}

forward!(&T, Arc<T>);

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
        Call {
            backend,
            contract_id,
            call,
        }
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
            Ok(b"{}".to_vec()),
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
    fn public_constructors_send_default_args_until_set() {
        // JSON sends the empty object, as a `#[contract]` view does.
        let viewer = MockViewer::new(b"");
        let value: Option<u64> =
            run(View::json(viewer.clone(), contract(), String::from("get"))).unwrap();
        assert_eq!(value, None);
        let (_, method, args, _) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!((method.as_str(), args), ("get", b"{}".to_vec()));

        // Borsh sends nothing, the encoding of `()`.
        let viewer = MockViewer::new(&borsh::to_vec(&9_u16).unwrap());
        let value: u16 = run(View::borsh(viewer.clone(), contract(), "get")).unwrap();
        assert_eq!(value, 9);
        let (_, _, args, _) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(args, Vec::<u8>::new());

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
    fn fetch_raw_returns_undecoded_bytes_and_decode_matches_await() {
        let viewer = MockViewer::new(b"42");
        let view = View::<_, u64>::json(viewer.clone(), contract(), "get")
            .args(serde_json::json!({ "k": 1 }))
            .at(5_u64);

        let raw = run(view.clone().fetch_raw()).unwrap();
        assert_eq!(raw.result, b"42");
        assert_eq!(
            (raw.block_height, raw.logs.as_slice()),
            (7, ["log".to_owned()].as_slice())
        );
        let (_, method, args, block) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(
            (method.as_str(), args.as_slice()),
            ("get", br#"{"k":1}"#.as_slice())
        );
        assert_eq!(block, BlockReference::Height(5));

        assert_eq!(view.decode(&raw.result).unwrap(), 42);
        assert_eq!(
            view.decode(&raw.result).unwrap(),
            run(view.clone()).unwrap()
        );
        assert!(matches!(view.decode(b"nope"), Err(CodecError::Json(_))));

        // JSON decoding reads empty bytes as `null`, as `.await` does.
        let optional = View::<_, Option<u64>>::json(viewer.clone(), contract(), "get");
        assert_eq!(optional.decode(b"").unwrap(), None);
        let borsh = View::<_, u32>::borsh(viewer.clone(), contract(), "get");
        assert_eq!(borsh.decode(&7_u32.to_le_bytes()).unwrap(), 7);
        assert!(matches!(borsh.decode(b""), Err(CodecError::Borsh(_))));
    }

    #[test]
    fn fetch_raw_keeps_error_layers() {
        let viewer = MockViewer::new(b"1");
        let error = run(View::<_, u64>::json(viewer.clone(), contract(), "get")
            .args(FailingJson)
            .fetch_raw())
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));
        assert!(viewer.seen.lock().unwrap().is_none(), "nothing was sent");

        let error =
            run(View::<_, u64>::json(viewer.clone(), contract(), "fail").fetch_raw()).unwrap_err();
        assert!(matches!(&error, ContractError::Backend(e) if e == "backend down"));

        // An undecodable result is not an error until it is decoded.
        let raw = run(View::<_, u64>::json(viewer, contract(), "get").fetch_raw()).unwrap();
        assert_eq!(raw.result, b"1");
    }

    #[test]
    fn a_cloned_view_runs_again() {
        let viewer = MockViewer::new(b"5");
        let view = View::<_, u64>::json(viewer.clone(), contract(), "get").at(9_u64);
        let first = view.clone();
        assert_eq!(run(first).unwrap(), 5);
        assert_eq!(run(view.clone()).unwrap(), 5);
        assert_eq!(run(view).unwrap(), 5);
        let (_, _, args, block) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(
            (args.as_slice(), block),
            (b"{}".as_slice(), BlockReference::Height(9))
        );
    }

    #[test]
    fn a_cloned_view_keeps_its_argument_encoding_error() {
        let viewer = MockViewer::new(b"1");
        let json = View::<_, u64>::json(viewer.clone(), contract(), "get").args(FailingJson);
        let original = json.args.as_ref().unwrap_err().to_string();
        let error = run(json.clone()).unwrap_err();
        assert!(matches!(&error, ContractError::Codec(CodecError::Json(_))));
        assert_eq!(error.to_string(), original);

        let borsh =
            View::<_, u64>::borsh(viewer.clone(), contract(), "get").args_borsh(FailingBorsh);
        let error = run(borsh.clone()).unwrap_err();
        let ContractError::Codec(CodecError::Borsh(io)) = &error else {
            panic!("expected a Borsh error, got {error:?}");
        };
        assert_eq!(io.kind(), std::io::ErrorKind::Other);
        assert_eq!(io.to_string(), "deliberate");
        assert!(viewer.seen.lock().unwrap().is_none(), "nothing was sent");
    }

    #[test]
    fn call_sends_one_function_call() {
        let sender = Arc::new(MockSender::default());
        let typed = || {
            call(sender.clone(), contract(), FunctionCall::new("add").args(5))
                .gas(Gas::from_tgas(50))
                .deposit(NearToken::from_yoctonear(1))
        };
        let expected = Action::FunctionCall(FunctionCallAction {
            method_name: "add".to_owned(),
            args: b"5".to_vec(),
            gas: Gas::from_tgas(50),
            deposit: NearToken::from_yoctonear(1),
        });

        // Converting into the function call sends nothing.
        assert_eq!(FunctionCall::from(typed()).into_action().unwrap(), expected);
        assert!(sender.sent.lock().unwrap().is_empty());

        assert_eq!(run(typed()).unwrap(), 1);
        let (receiver, actions) = sender.sent.lock().unwrap().pop().unwrap();
        assert_eq!(receiver, contract());
        assert_eq!(actions, [expected]);
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

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn futures_are_send_and_static() {
        fn assert_spawnable<F: Future + Send + 'static>(_: F) {}
        fn generic<V, S>(viewer: V, sender: S)
        where
            V: Viewer + Clone + 'static,
            S: TxSender + 'static,
            V::ViewError: Send,
            S::Output: Send,
            S::SendError: Send,
        {
            let view = view_json::<_, u64>(viewer, contract(), "get", Ok(Vec::new()));
            assert_spawnable(view.clone().fetch_raw());
            assert_spawnable(view.into_future());
            assert_spawnable(call(sender, contract(), FunctionCall::new("add")).into_future());
        }
        generic(MockViewer::new(b"1"), Arc::new(MockSender::default()));
    }

    /// A `!Send` backend error doesn't make the view futures `!Send`.
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn futures_are_send_with_a_non_send_view_error() {
        fn assert_send<F: Future + Send>(_: F) {}
        #[derive(Clone)]
        struct RcError;
        impl Viewer for RcError {
            type ViewError = std::rc::Rc<str>;
            async fn view_function(
                &self,
                _: &AccountId,
                _: &str,
                _: &[u8],
                _: BlockReference,
            ) -> Result<ViewFunctionResult, Self::ViewError> {
                Ok(ViewFunctionResult::new(Vec::new(), 0, CryptoHash::ZERO))
            }
        }
        let view = view_json::<_, u64>(RcError, contract(), "get", Ok(Vec::new()));
        assert_send(view.clone().fetch_raw());
        assert_send(view.with_metadata());
    }

    #[test]
    fn borrowed_backends_forward() {
        let viewer = MockViewer::new(b"5");
        let by_ref: u64 = futures::executor::block_on(
            view_json::<_, u64>(&*viewer, contract(), "get", Ok(Vec::new())).with_metadata(),
        )
        .unwrap()
        .result;
        assert_eq!(by_ref, 5);
    }

    #[test]
    fn call_sends_through_a_local_borrow() {
        // `.await` would need `&sender` to be `'static`; `send` doesn't.
        let sender = MockSender::default();
        let sent =
            futures::executor::block_on(call(&sender, contract(), FunctionCall::new("add")).send())
                .unwrap();
        assert_eq!(sent, 1);
        let recorded = sender.sent.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].0, contract());

        // An argument encoding error still surfaces as `Codec`.
        let failing = call(
            &sender,
            contract(),
            FunctionCall::new("add").args(FailingJson),
        );
        drop(recorded);
        assert!(matches!(
            futures::executor::block_on(failing.send()),
            Err(ContractError::Codec(_))
        ));
    }
}
