//! One backend picked at run time for both views and calls: an object-safe
//! trait in front of `Viewer` and `TxSender`, from near-kit's public API.
//!
//! `impl Viewer for dyn _` awaits the boxed future inside an `async fn`:
//! `view_function` borrows four arguments with independent lifetimes, and
//! returning the `BoxFuture` directly fails with E0700 (its single lifetime
//! is their intersection, which the opaque return type can't name).
#![cfg(all(feature = "contracts", feature = "rpc"))]
use std::future::Future;
use std::sync::Arc;

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, BoxFuture, SendTxResponse, ViewFunctionResult};
use near_kit::*;

type DynError = Box<dyn std::error::Error + Send + Sync>;

trait DynBackend: MaybeSend + MaybeSync {
    fn dyn_view<'a>(
        &'a self,
        contract_id: &'a AccountIdRef,
        method: &'a str,
        args: &'a [u8],
        block: BlockReference,
    ) -> BoxFuture<'a, Result<ViewFunctionResult, DynError>>;
    fn dyn_account_id(&self) -> &AccountId;
    fn dyn_chain_id(&self) -> &str;
    fn dyn_send(
        &self,
        r: AccountId,
        a: Vec<Action>,
    ) -> BoxFuture<'_, Result<SendTxResponse, DynError>>;
}

impl<B> DynBackend for B
where
    B: Viewer + TxSender,
    B::ViewError: Into<DynError>,
    B::SendError: Into<DynError>,
{
    fn dyn_view<'a>(
        &'a self,
        contract_id: &'a AccountIdRef,
        method: &'a str,
        args: &'a [u8],
        block: BlockReference,
    ) -> BoxFuture<'a, Result<ViewFunctionResult, DynError>> {
        Box::pin(async move {
            self.view_function(contract_id, method, args, block)
                .await
                .map_err(Into::into)
        })
    }
    fn dyn_account_id(&self) -> &AccountId {
        self.account_id()
    }
    fn dyn_chain_id(&self) -> &str {
        self.chain_id()
    }
    fn dyn_send(
        &self,
        r: AccountId,
        a: Vec<Action>,
    ) -> BoxFuture<'_, Result<SendTxResponse, DynError>> {
        Box::pin(async move { self.send(r, a).await.map(Into::into).map_err(Into::into) })
    }
}

impl Viewer for dyn DynBackend + '_ {
    type ViewError = DynError;
    // An `async fn`, not `-> impl Future { self.dyn_view(..) }`: see the
    // module docs (E0700).
    async fn view_function(
        &self,
        contract_id: &AccountIdRef,
        method: &str,
        args: &[u8],
        block: BlockReference,
    ) -> Result<ViewFunctionResult, DynError> {
        self.dyn_view(contract_id, method, args, block).await
    }
}

impl TxSender for dyn DynBackend + '_ {
    type Output = SendTxResponse;
    type SendError = DynError;
    fn account_id(&self) -> &AccountId {
        self.dyn_account_id()
    }
    fn chain_id(&self) -> &str {
        self.dyn_chain_id()
    }
    fn send(
        &self,
        r: AccountId,
        a: Vec<Action>,
    ) -> impl Future<Output = Result<SendTxResponse, DynError>> + MaybeSend {
        self.dyn_send(r, a)
    }
}

#[near_kit::contract]
pub trait Counter {
    fn get(&self) -> u64;
    #[call]
    fn increment(&mut self);
}

#[derive(Debug)]
struct Offline;
impl std::fmt::Display for Offline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("offline")
    }
}
impl std::error::Error for Offline {}

struct Mock(AccountId);
impl Viewer for Mock {
    // `ViewError` is unbounded; this one erases into the boxed error.
    type ViewError = Offline;
    async fn view_function(
        &self,
        _c: &AccountIdRef,
        method: &str,
        _a: &[u8],
        _b: BlockReference,
    ) -> Result<ViewFunctionResult, Offline> {
        if method == "get" {
            Ok(ViewFunctionResult::new(b"7".to_vec(), 1, CryptoHash::ZERO))
        } else {
            Err(Offline)
        }
    }
}
impl TxSender for Mock {
    type Output = SendTxResponse;
    type SendError = near_kit::transaction::CodecError;
    fn account_id(&self) -> &AccountId {
        &self.0
    }
    fn chain_id(&self) -> &str {
        "testnet"
    }
    async fn send(
        &self,
        _r: AccountId,
        _a: Vec<Action>,
    ) -> Result<SendTxResponse, Self::SendError> {
        Ok(SendTxResponse::new(CryptoHash::ZERO, self.0.clone(), None))
    }
}

fn runtime_choice(near: Option<SigningNear>) -> Arc<dyn DynBackend> {
    match near {
        Some(near) => Arc::new(near),
        None => Arc::new(Mock(account!("alice.testnet"))),
    }
}

#[tokio::test]
async fn one_dyn_backend_views_and_calls() {
    let backend = runtime_choice(None);
    let counter = Counter::client(backend, account!("counter.testnet"));
    assert_eq!(counter.get().await.unwrap(), 7);
    let sent = counter.increment().await.unwrap();
    assert_eq!(sent.sender_id, "alice.testnet");
    let raw: Result<ViewFunctionResult, _> =
        View::<_, u64>::json(runtime_choice(None), account!("c.testnet"), "nope")
            .fetch_raw()
            .await;
    match raw.unwrap_err() {
        ContractError::Backend(e) => assert!(e.downcast_ref::<Offline>().is_some()),
        other => panic!("{other:?}"),
    }
}
