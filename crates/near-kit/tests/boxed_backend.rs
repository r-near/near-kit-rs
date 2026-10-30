//! A `Box` around a backend is a backend: `Box<T>` forwards `Viewer` and
//! `TxSender` to `T`, like `&T` and `Arc<T>`, so a boxed `Clone` backend
//! drives a generated client.
#![cfg(feature = "contracts")]

use std::sync::{Arc, Mutex};

use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::{AccountId, Contract, CryptoHash, TxSender, Viewer, account};

#[near_kit::contract]
pub trait Counter {
    fn get(&self) -> u64;

    #[call]
    fn increment(&mut self);
}

/// Records the method of every view and call; views answer `7`, calls the
/// number of actions sent.
#[derive(Clone)]
struct Recorder {
    account_id: AccountId,
    methods: Arc<Mutex<Vec<String>>>,
}

impl Viewer for Recorder {
    type ViewError = std::convert::Infallible;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, Self::ViewError> {
        self.methods.lock().unwrap().push(method.to_owned());
        Ok(ViewFunctionResult::new(b"7".to_vec(), 1, CryptoHash::ZERO))
    }
}

impl TxSender for Recorder {
    type Output = usize;
    type SendError = std::convert::Infallible;

    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    async fn send(
        &self,
        _receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<usize, Self::SendError> {
        for action in &actions {
            let Action::FunctionCall(call) = action else {
                panic!("expected a function call, got {action:?}");
            };
            self.methods.lock().unwrap().push(call.method_name.clone());
        }
        Ok(actions.len())
    }
}

#[tokio::test]
async fn a_boxed_backend_drives_a_contract_view_and_call() {
    let recorder = Recorder {
        account_id: account!("alice.testnet"),
        methods: Arc::default(),
    };
    let backend: Box<Recorder> = Box::new(recorder.clone());
    assert_eq!(TxSender::account_id(&backend).as_str(), "alice.testnet");

    let counter = Counter::client(backend, account!("counter.testnet"));
    assert_eq!(counter.get().await.unwrap(), 7);
    assert_eq!(counter.increment().await.unwrap(), 1);

    assert_eq!(*recorder.methods.lock().unwrap(), ["get", "increment"]);
}
