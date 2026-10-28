//! The method name a generated client sends on the wire.

use std::sync::{Arc, Mutex};

use near_kit::protocol::{Action, FunctionCallAction};
use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::*;

/// Methods named after Rust keywords, declared with raw identifiers.
#[near_kit::contract]
pub trait Keywords {
    fn r#type(&self) -> u64;

    #[call]
    fn r#move(&mut self);

    #[call]
    #[borsh]
    fn r#match(&mut self, value: u64);
}

/// Records the method names a client sends.
#[derive(Clone, Default)]
struct Recorder {
    methods: Arc<Mutex<Vec<String>>>,
}

impl Recorder {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.methods.lock().unwrap())
    }
}

impl Viewer for Recorder {
    type ViewError = String;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        method: &str,
        _args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, String> {
        self.methods.lock().unwrap().push(method.to_owned());
        Err("recorded".to_owned())
    }
}

impl TxSender for Recorder {
    type Output = ();
    type SendError = String;

    fn account_id(&self) -> &AccountId {
        unreachable!("the generated client doesn't ask for the sender")
    }

    async fn send(&self, _receiver_id: AccountId, actions: Vec<Action>) -> Result<(), String> {
        let mut methods = self.methods.lock().unwrap();
        for action in actions {
            match action {
                Action::FunctionCall(call) => methods.push(call.method_name),
                other => panic!("expected a function call, got {other:?}"),
            }
        }
        Ok(())
    }
}

fn method_name(call: transaction::FunctionCall) -> String {
    FunctionCallAction::try_from(call).unwrap().method_name
}

#[test]
fn raw_identifier_constructors_use_the_unprefixed_name() {
    assert_eq!(method_name(Keywords::r#move()), "move");
    assert_eq!(method_name(Keywords::r#match(7)), "match");
}

#[tokio::test]
async fn raw_identifier_client_methods_use_the_unprefixed_name() {
    let recorder = Recorder::default();
    let client = Keywords::client(recorder.clone(), account!("keywords.near"));

    let _ = client.r#type().await;
    client.r#move().await.unwrap();
    client.r#match(7).await.unwrap();

    assert_eq!(recorder.take(), ["type", "move", "match"]);
}
