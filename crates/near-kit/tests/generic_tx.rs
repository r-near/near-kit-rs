//! `Tx` over any `TxSender`: through a borrow, owned and spawned, through a
//! wrapper sender, with a per-transaction wait level, and with an argument
//! that fails to encode. The receiver shapes at the end only have to compile.
#![cfg(feature = "rpc")]

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use near_kit::protocol::{Action, ChainId};
use near_kit::rpc::{SendTxResponse, TxExecutionStatus};
use near_kit::transaction::{CodecError, Final, FunctionCall, Included, WaitLevel};
use near_kit::{
    AccountId, CryptoHash, Error, Gas, NearToken, SigningNear, Tx, TxSender, WithWaitLevel, account,
};
use serde_json::json;

/// A transaction as the mock saw it, with the level it was sent at.
type Sent = (AccountId, Vec<Action>, TxExecutionStatus);

/// Records every transaction and answers with a placeholder response. It
/// never fails, so its error is `CodecError`, the smallest error that takes
/// an encoding error.
struct Mock<W: WaitLevel = Final> {
    account_id: AccountId,
    chain_id: ChainId,
    sent: Arc<Mutex<Vec<Sent>>>,
    _wait: PhantomData<fn() -> W>,
}

impl Mock {
    fn new() -> Self {
        Self {
            account_id: account!("alice.testnet"),
            chain_id: ChainId::testnet(),
            sent: Arc::default(),
            _wait: PhantomData,
        }
    }
}

impl<W: WaitLevel> Mock<W> {
    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }
}

impl<W: WaitLevel> TxSender for Mock<W> {
    type Output = SendTxResponse;
    type SendError = CodecError;

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }

    async fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<SendTxResponse, CodecError> {
        self.sent
            .lock()
            .unwrap()
            .push((receiver_id, actions, W::STATUS));
        Ok(SendTxResponse::new(
            CryptoHash::ZERO,
            self.account_id.clone(),
            None,
        ))
    }
}

/// The same mock, recording another level: what `Tx::wait_until` uses.
impl<W: WaitLevel> WithWaitLevel for Mock<W> {
    type At<L: WaitLevel> = Mock<L>;

    fn wait_until<L: WaitLevel>(&self) -> Mock<L> {
        Mock {
            account_id: self.account_id.clone(),
            chain_id: self.chain_id.clone(),
            sent: self.sent.clone(),
            _wait: PhantomData,
        }
    }
}

fn counter() -> AccountId {
    account!("counter.testnet")
}

/// A function call and a tip in one transaction, through any sender.
async fn bump<S: TxSender>(sender: &S) -> Result<S::Output, S::SendError> {
    sender
        .transaction(counter())
        .add_action(
            FunctionCall::new("bump")
                .args(json!({ "by": 2 }))
                .gas(Gas::from_tgas(10)),
        )
        .add_action(Action::transfer(NearToken::from_millinear(10)))
        .await
}

fn assert_bumped(sent: &[Sent]) {
    let [(receiver_id, actions, _)] = sent else {
        panic!("expected one transaction, got {sent:?}");
    };
    assert_eq!(*receiver_id, counter());
    let [Action::FunctionCall(call), Action::Transfer(tip)] = actions.as_slice() else {
        panic!("expected a call and a transfer, got {actions:?}");
    };
    assert_eq!(call.method_name, "bump");
    assert_eq!(call.args, br#"{"by":2}"#);
    assert_eq!(tip.deposit, NearToken::from_millinear(10));
}

#[tokio::test]
async fn a_generic_sender_sends_through_a_borrow() {
    let mock = Mock::new();
    let sent = bump(&mock).await.unwrap();
    assert_eq!(sent.sender_id, "alice.testnet");
    assert_bumped(&mock.sent());
}

#[tokio::test]
async fn an_owned_transaction_spawns() {
    fn spawn<S>(sender: S) -> tokio::task::JoinHandle<Result<S::Output, S::SendError>>
    where
        S: TxSender + Send + Sync + 'static,
        S::Output: Send + 'static,
        S::SendError: Send + 'static,
    {
        let tx = Tx::new(sender, counter())
            .add_action(FunctionCall::new("bump").args(json!({ "by": 2 })))
            .add_action(Action::transfer(NearToken::from_millinear(10)));
        tokio::spawn(tx.into_future())
    }

    let mock = Mock::new();
    let recorded = mock.sent.clone();
    spawn(mock).await.unwrap().unwrap();
    assert_eq!(recorded.lock().unwrap().len(), 1);
}

/// A sender that wraps another and refuses to deploy contracts.
struct Wallet<S> {
    inner: S,
}

#[derive(Debug)]
enum WalletError<E> {
    Unsupported(&'static str),
    Inner(E),
}

impl<E: fmt::Display> fmt::Display for WalletError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(what) => write!(f, "the wallet doesn't support {what}"),
            Self::Inner(error) => error.fmt(f),
        }
    }
}

/// Every `SendError` takes an encoding error; this one hands it to the
/// inner sender's.
impl<E: From<CodecError>> From<CodecError> for WalletError<E> {
    fn from(error: CodecError) -> Self {
        Self::Inner(E::from(error))
    }
}

impl<S: TxSender> TxSender for Wallet<S> {
    type Output = SendTxResponse;
    type SendError = WalletError<S::SendError>;

    fn account_id(&self) -> &AccountId {
        self.inner.account_id()
    }

    fn chain_id(&self) -> &ChainId {
        self.inner.chain_id()
    }

    async fn send(
        &self,
        receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<SendTxResponse, Self::SendError> {
        if actions
            .iter()
            .any(|action| matches!(action, Action::DeployContract(_)))
        {
            return Err(WalletError::Unsupported("deploying contracts"));
        }
        match self.inner.send(receiver_id, actions).await {
            Ok(output) => Ok(output.into()),
            Err(error) => Err(WalletError::Inner(error)),
        }
    }
}

/// The wallet at another level is the wallet around the inner sender at
/// that level.
impl<S: WithWaitLevel> WithWaitLevel for Wallet<S> {
    type At<L: WaitLevel> = Wallet<S::At<L>>;

    fn wait_until<L: WaitLevel>(&self) -> Self::At<L> {
        Wallet {
            inner: self.inner.wait_until::<L>(),
        }
    }
}

#[tokio::test]
async fn a_wrapper_sender_builds_the_same_transaction() {
    let mock = Mock::new();
    let recorded = mock.sent.clone();
    let wallet = Wallet { inner: mock };

    bump(&wallet).await.unwrap();
    assert_bumped(&recorded.lock().unwrap());

    let error = wallet
        .transaction(counter())
        .add_action(Action::deploy_contract(b"\0asm".to_vec()))
        .await
        .unwrap_err();
    assert!(matches!(error, WalletError::Unsupported(_)), "{error:?}");
    assert_eq!(recorded.lock().unwrap().len(), 1, "the deploy wasn't sent");
}

#[tokio::test]
async fn wait_until_picks_the_level_of_one_transaction() {
    let mock = Mock::new();
    mock.transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .wait_until::<Included>()
        .await
        .unwrap();
    // Through a wrapper too.
    let wallet = Wallet {
        inner: Mock {
            sent: mock.sent.clone(),
            ..Mock::new()
        },
    };
    wallet
        .transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .wait_until::<Included>()
        .await
        .unwrap();
    // The sender keeps its own level.
    mock.transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await
        .unwrap();

    let levels: Vec<_> = mock.sent().into_iter().map(|(_, _, level)| level).collect();
    assert_eq!(
        levels,
        [
            TxExecutionStatus::Included,
            TxExecutionStatus::Included,
            TxExecutionStatus::Final
        ]
    );
}

#[tokio::test]
async fn wait_until_through_a_borrow_can_be_spawned() {
    let mock = Mock::new();
    // `&Mock` forwards `WithWaitLevel`, so the result owns a `Mock<Included>`.
    let tx = mock
        .transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .wait_until::<Included>();
    tokio::spawn(tx.into_future()).await.unwrap().unwrap();
    assert_eq!(mock.sent().len(), 1);
}

/// JSON object keys must be strings, so these arguments fail to encode.
fn unencodable_args() -> BTreeMap<(u8, u8), u8> {
    BTreeMap::from([((1, 2), 3)])
}

#[tokio::test]
async fn an_encoding_error_is_the_senders_error_and_nothing_is_sent() {
    let mock = Mock::new();
    let error = mock
        .transaction(counter())
        .add_action(FunctionCall::new("init").args(unencodable_args()))
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await
        .unwrap_err();
    assert!(matches!(error, CodecError::Json(_)), "{error:?}");
    assert!(mock.sent().is_empty());

    // Routed through the wrapper's `From<CodecError>`.
    let wallet = Wallet { inner: Mock::new() };
    let error = wallet
        .transaction(counter())
        .add_action(FunctionCall::new("init").args(unencodable_args()))
        .await
        .unwrap_err();
    assert!(
        matches!(error, WalletError::Inner(CodecError::Json(_))),
        "{error:?}"
    );
}

// Receiver shapes: each picks `SigningNear::transaction` (owned) or
// `TxSender::transaction` (borrowed), and all of them await.

#[allow(dead_code)]
async fn owned_and_borrowed_signing_near(alice: SigningNear) -> Result<(), Error> {
    let by_value: Tx<'static, SigningNear> = alice.transaction(counter());
    let borrowed: &SigningNear = &alice;
    let by_ref: Tx<'static, SigningNear> = borrowed.transaction(counter());
    tokio::spawn(by_value.into_future());
    by_ref.await?.result()?;
    Ok(())
}

#[allow(dead_code)]
async fn arc_and_box(alice: Arc<SigningNear>, bob: Box<SigningNear>) -> Result<(), Error> {
    alice
        .transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .await?
        .result()?;
    let _: SendTxResponse = bob
        .transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .wait_until::<Included>()
        .await?;
    // The owned form, to spawn.
    tokio::spawn((*alice).transaction(counter()).into_future());
    Ok(())
}

#[allow(dead_code)]
async fn double_references(signers: &[&SigningNear]) -> Result<(), Error> {
    for signer in signers {
        signer
            .transaction(counter())
            .add_action(Action::transfer(NearToken::from_near(1)))
            .await?
            .result()?;
    }
    Ok(())
}

#[allow(dead_code)]
struct App {
    alice: SigningNear,
}

#[allow(dead_code)]
impl App {
    async fn pay(&self) -> Result<(), Error> {
        let tx: Tx<'static, SigningNear> = self.alice.transaction(counter());
        tx.add_action(Action::transfer(NearToken::from_near(1)))
            .await?
            .result()?;
        Ok(())
    }
}

#[allow(dead_code)]
async fn generic_wait_level<S: WithWaitLevel>(
    sender: &S,
) -> Result<<S::At<Included> as TxSender>::Output, S::SendError> {
    sender
        .transaction(counter())
        .add_action(Action::transfer(NearToken::from_near(1)))
        .wait_until::<Included>()
        .await
}
