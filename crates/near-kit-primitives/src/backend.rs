//! The two chain verbs ([`Viewer`], [`TxSender`]) and the awaitable values
//! that generated contract clients return ([`View`], [`Call`]).
//!
//! Nothing here names an RPC client: `near-kit`'s `Near` implements these
//! traits, and so can any other backend (a gateway, a test mock, a sender
//! that relays through someone else's account).

use std::fmt;
use std::future::{Future, IntoFuture};

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
///     type Error = std::convert::Infallible;
///
///     async fn view_function(
///         &self,
///         _contract_id: &AccountId,
///         _method: &str,
///         _args: &[u8],
///         _block: BlockReference,
///     ) -> Result<ViewFunctionResult, Self::Error> {
///         Ok(ViewFunctionResult {
///             result: b"42".to_vec(),
///             logs: Vec::new(),
///             block_height: 1,
///             block_hash: CryptoHash::ZERO,
///         })
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
    type Error;

    /// Run `method` on `contract_id` with already-encoded `args`, reading the
    /// state at `block`.
    fn view_function(
        &self,
        contract_id: &AccountId,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> impl Future<Output = Result<ViewFunctionResult, Self::Error>> + MaybeSend;
}

/// A backend that can send transactions from a fixed account.
///
/// This is the write verb: a transaction is a receiver plus actions, and the
/// backend decides how to sign, how long to wait and what to return
/// ([`Output`](Self::Output)).
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot send transactions",
    label = "needs a signer",
    note = "attach a signer with `near.with_signer(signer)`"
)]
pub trait TxSender: MaybeSend + MaybeSync {
    /// What a sent transaction resolves to, such as an execution outcome or
    /// just a transaction hash.
    type Output;
    /// The backend's error. Unbounded.
    type Error;

    /// The account transactions are sent from.
    fn account_id(&self) -> &AccountId;

    /// Send one transaction with `actions` to `receiver_id`.
    fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + MaybeSend;
}

/// Error from a [`View`] or [`Call`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ContractError<E> {
    /// The arguments failed to encode, or the view result failed to decode.
    #[error(transparent)]
    Codec(CodecError),
    /// The backend failed.
    #[error(transparent)]
    Backend(E),
}

/// A typed contract view, ready to `.await`.
///
/// Returned by the view methods of a `#[contract]` client. Reads at
/// [`BlockReference::default()`] (final) unless [`at`](Self::at) says
/// otherwise. Awaiting it yields the decoded value; use
/// [`with_metadata`](Self::with_metadata) to also get the block it was read
/// at.
#[must_use = "a view does nothing until it is awaited"]
pub struct View<'a, C, T> {
    backend: &'a C,
    contract_id: AccountId,
    method: &'static str,
    // Arguments are encoded when the view is created, so borrowed argument
    // types never end up inside the future.
    args: Result<Vec<u8>, CodecError>,
    block: BlockReference,
    decode: fn(&[u8]) -> Result<T, CodecError>,
}

impl<'a, C, T> View<'a, C, T> {
    pub(crate) fn new(
        backend: &'a C,
        contract_id: AccountId,
        method: &'static str,
        args: Result<Vec<u8>, CodecError>,
        decode: fn(&[u8]) -> Result<T, CodecError>,
    ) -> Self {
        Self {
            backend,
            contract_id,
            method,
            args,
            block: BlockReference::default(),
            decode,
        }
    }

    /// Read at a specific block height, hash or finality.
    pub fn at(mut self, block: impl Into<BlockReference>) -> Self {
        self.block = block.into();
        self
    }

    /// Run the view and keep the block height, block hash and logs alongside
    /// the decoded value.
    pub async fn with_metadata(self) -> Result<ViewFunctionResult<T>, ContractError<C::Error>>
    where
        C: Viewer,
    {
        let args = match self.args {
            Ok(args) => args,
            Err(error) => return Err(ContractError::Codec(error)),
        };
        // `match` rather than `?` on the backend result: a `?` temporary
        // holding `C::Error` would make the future `!Send` for a `!Send`
        // error.
        let raw = match self
            .backend
            .view_function(&self.contract_id, self.method, &args, self.block)
            .await
        {
            Ok(raw) => raw,
            Err(error) => return Err(ContractError::Backend(error)),
        };
        Ok(ViewFunctionResult {
            result: (self.decode)(&raw.result).map_err(ContractError::Codec)?,
            logs: raw.logs,
            block_height: raw.block_height,
            block_hash: raw.block_hash,
        })
    }
}

impl<C, T> fmt::Debug for View<'_, C, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("View")
            .field("contract_id", &self.contract_id)
            .field("method", &self.method)
            .field("args", &self.args.as_ref().map(Vec::len))
            .field("block", &self.block)
            .finish_non_exhaustive()
    }
}

impl<'a, C, T> IntoFuture for View<'a, C, T>
where
    C: Viewer,
    C::Error: 'a,
    T: 'a,
{
    type Output = Result<T, ContractError<C::Error>>;
    type IntoFuture = BoxFuture<'a, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move { self.with_metadata().await.map(|view| view.result) })
    }
}

/// A typed contract call, ready to `.await`.
///
/// Returned by the `#[call]` methods of a `#[contract]` client. Awaiting it
/// sends one transaction with this function call through the client's
/// [`TxSender`] and resolves to the sender's [`Output`](TxSender::Output).
#[must_use = "a call does nothing until it is awaited"]
pub struct Call<'a, C> {
    backend: &'a C,
    contract_id: AccountId,
    call: FunctionCall,
}

impl<'a, C> Call<'a, C> {
    pub(crate) fn new(backend: &'a C, contract_id: AccountId, call: FunctionCall) -> Self {
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

impl<C> fmt::Debug for Call<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Call")
            .field("contract_id", &self.contract_id)
            .field("call", &self.call)
            .finish_non_exhaustive()
    }
}

impl<'a, C> IntoFuture for Call<'a, C>
where
    C: TxSender,
    C::Output: 'a,
    C::Error: 'a,
{
    type Output = Result<C::Output, ContractError<C::Error>>;
    type IntoFuture = BoxFuture<'a, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        // Start the send now: the boxed future then holds only the backend's
        // own future. Fully qualified, so an inherent `send` on the backend
        // can't shadow the trait method.
        let sent = Action::try_from(self.call)
            .map(|action| TxSender::send(self.backend, self.contract_id, vec![action]));
        Box::pin(async move {
            match sent {
                Ok(sent) => match sent.await {
                    Ok(output) => Ok(output),
                    Err(error) => Err(ContractError::Backend(error)),
                },
                Err(error) => Err(ContractError::Codec(error)),
            }
        })
    }
}

/// Constructors and encoders used by `#[contract]` expansions (and by
/// near-kit's token clients). Not public API.
pub(crate) mod expansion {
    use super::*;

    /// A JSON-decoded view.
    pub fn view_json<'a, C, T>(
        backend: &'a C,
        contract_id: AccountId,
        method: &'static str,
        args: Result<Vec<u8>, CodecError>,
    ) -> View<'a, C, T>
    where
        C: Viewer,
        T: serde::de::DeserializeOwned,
    {
        View::new(backend, contract_id, method, args, |bytes| {
            // Void methods return no bytes; read them as `null` so `()` and
            // `Option<T>` decode.
            let bytes = if bytes.is_empty() { b"null" } else { bytes };
            Ok(serde_json::from_slice(bytes)?)
        })
    }

    /// A Borsh-decoded view.
    pub fn view_borsh<'a, C, T>(
        backend: &'a C,
        contract_id: AccountId,
        method: &'static str,
        args: Result<Vec<u8>, CodecError>,
    ) -> View<'a, C, T>
    where
        C: Viewer,
        T: borsh::BorshDeserialize,
    {
        View::new(backend, contract_id, method, args, |bytes| {
            borsh::from_slice(bytes).map_err(CodecError::Borsh)
        })
    }

    /// A call through `backend`.
    pub fn call<C>(backend: &C, contract_id: AccountId, call: FunctionCall) -> Call<'_, C>
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
        fn new(answer: &[u8]) -> Self {
            Self {
                answer: answer.to_vec(),
                seen: Mutex::new(None),
            }
        }
    }

    impl Viewer for MockViewer {
        type Error = String;

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
            Ok(ViewFunctionResult {
                result: self.answer.clone(),
                logs: vec!["log".to_owned()],
                block_height: 7,
                block_hash: CryptoHash::ZERO,
            })
        }
    }

    /// Records sent transactions and returns how many actions it got.
    #[derive(Default)]
    struct MockSender {
        sent: Mutex<Vec<(AccountId, Vec<Action>)>>,
    }

    impl TxSender for MockSender {
        type Output = usize;
        type Error = String;

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
            &viewer,
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
            run(view_json(&viewer, contract(), "get", Ok(Vec::new()))).unwrap();
        assert_eq!(value, None);
        run(view_json::<_, ()>(
            &viewer,
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
        let value: u32 = run(view_borsh(&viewer, contract(), "get", args)).unwrap();
        assert_eq!(value, 7);
        let (_, _, args, _) = viewer.seen.lock().unwrap().take().unwrap();
        assert_eq!(args, [1]);
    }

    #[test]
    fn at_and_with_metadata() {
        let viewer = MockViewer::new(b"\"hi\"");
        let result = run(
            view_json::<_, String>(&viewer, contract(), "get", Ok(Vec::new()))
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
            &viewer,
            contract(),
            "get",
            json_args(&FailingJson),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));
        assert!(viewer.seen.lock().unwrap().is_none(), "nothing was sent");

        let error = run(view_json::<_, u64>(
            &viewer,
            contract(),
            "fail",
            Ok(Vec::new()),
        ))
        .unwrap_err();
        assert!(matches!(&error, ContractError::Backend(e) if e == "backend down"));

        let error = run(view_json::<_, u64>(
            &viewer,
            contract(),
            "get",
            Ok(Vec::new()),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));

        let error = run(view_borsh::<_, u64>(
            &viewer,
            contract(),
            "get",
            Ok(Vec::new()),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Borsh(_))));
    }

    #[test]
    fn call_sends_one_function_call() {
        let sender = MockSender::default();
        let count = run(call(&sender, contract(), FunctionCall::new("add").args(5))
            .gas(Gas::from_tgas(50))
            .deposit(NearToken::from_yoctonear(1)))
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
        let sender = MockSender::default();
        let error = run(call(
            &sender,
            contract(),
            FunctionCall::new("add").args(FailingJson),
        ))
        .unwrap_err();
        assert!(matches!(error, ContractError::Codec(CodecError::Json(_))));
        assert!(sender.sent.lock().unwrap().is_empty());
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
    fn futures_are_send() {
        fn assert_send<F: Future + Send>(_: F) {}
        fn generic<V: Viewer, S: TxSender>(viewer: &V, sender: &S)
        where
            V::Error: Send,
            S::Output: Send,
            S::Error: Send,
        {
            assert_send(
                view_json::<_, u64>(viewer, contract(), "get", Ok(Vec::new())).into_future(),
            );
            assert_send(call(sender, contract(), FunctionCall::new("add")).into_future());
        }
        generic(&MockViewer::new(b"1"), &MockSender::default());
    }
}
