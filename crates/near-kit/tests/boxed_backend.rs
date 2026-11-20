//! A `Box` around a backend is a backend: `Box<T>` forwards `Viewer` and
//! `TxSender` to `T`, like `&T` and `Arc<T>`, so a boxed `Clone` backend
//! drives a generated client.
#![cfg(feature = "contracts")]

use std::sync::{Arc, Mutex};

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, SendTxResponse, ViewFunctionResult};
use near_kit::{AccountId, AccountIdRef, Contract, CryptoHash, TxSender, Viewer, account};

#[near_kit::contract]
pub trait Counter {
    fn get(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

/// Records the method of every view and call; views answer `7`, calls a
/// placeholder transaction from the recorder's account.
#[derive(Clone)]
struct Recorder {
    account_id: AccountId,
    chain_id: String,
    methods: Arc<Mutex<Vec<String>>>,
}

impl Viewer for Recorder {
    type ViewError = std::convert::Infallible;

    async fn view_function(
        &self,
        _contract_id: &AccountIdRef,
        method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, Self::ViewError> {
        self.methods.lock().unwrap().push(method.to_owned());
        Ok(ViewFunctionResult::new(b"7".to_vec(), 1, CryptoHash::ZERO))
    }
}

impl TxSender for Recorder {
    type Output = SendTxResponse;
    type SendError = near_kit::transaction::CodecError;

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn chain_id(&self) -> &str {
        &self.chain_id
    }

    async fn send(
        &self,
        _receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<SendTxResponse, Self::SendError> {
        for action in &actions {
            let Action::FunctionCall(call) = action else {
                panic!("expected a function call, got {action:?}");
            };
            self.methods.lock().unwrap().push(call.method_name.clone());
        }
        Ok(SendTxResponse::new(
            CryptoHash::ZERO,
            self.account_id.clone(),
            None,
        ))
    }
}

#[tokio::test]
async fn a_boxed_backend_drives_a_contract_view_and_call() {
    let recorder = Recorder {
        account_id: account!("alice.testnet"),
        chain_id: "testnet".to_owned(),
        methods: Arc::default(),
    };
    let backend: Box<Recorder> = Box::new(recorder.clone());
    assert_eq!(TxSender::account_id(&backend).as_str(), "alice.testnet");
    assert_eq!(TxSender::chain_id(&backend), "testnet");

    let counter = Counter::client(backend, account!("counter.testnet"));
    assert_eq!(counter.get().await.unwrap(), 7);
    let sent = counter.increment().await.unwrap();
    assert_eq!(sent.sender_id, "alice.testnet");

    assert_eq!(*recorder.methods.lock().unwrap(), ["get", "increment"]);
}
