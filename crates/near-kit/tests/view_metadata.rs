#![cfg(feature = "rpc")]

use std::sync::{Arc, Mutex};

use near_kit::rpc::{BoxFuture, RpcError, RpcTransport, TransportResponse, ViewFunctionResult};
use near_kit::{CryptoHash, Error, Near};
use serde_json::{Value, json};

type Requests = Arc<Mutex<Vec<Value>>>;

struct ResponseTransport {
    response: Vec<u8>,
    requests: Requests,
}

impl RpcTransport for ResponseTransport {
    fn post_json(
        &self,
        _url: &str,
        body: Vec<u8>,
    ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
        self.requests
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&body).unwrap());
        let body = self.response.clone();
        Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
    }
}

fn client(bytes: Vec<u8>) -> (Near, Requests) {
    let requests = Requests::default();
    let response = serde_json::to_vec(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "result": bytes,
            "logs": ["view evaluated"],
            "block_height": 42,
            "block_hash": CryptoHash::from_bytes([7; 32]),
        }
    }))
    .unwrap();
    let near = Near::testnet()
        .transport(ResponseTransport {
            response,
            requests: requests.clone(),
        })
        .build();
    (near, requests)
}

fn assert_metadata<T>(result: &ViewFunctionResult<T>) {
    assert_eq!(result.block_height, 42);
    assert_eq!(result.block_hash, CryptoHash::from_bytes([7; 32]));
    assert_eq!(result.logs, ["view evaluated"]);
}

#[tokio::test]
async fn json_value_and_metadata_come_from_one_selected_block_query() {
    let (near, requests) = client(b"123".to_vec());
    let result = near
        .view::<u64>("counter.testnet", "get_count")
        .args_raw(b"{}".to_vec())
        .at_block_hash(CryptoHash::from_bytes([7; 32]))
        .with_metadata()
        .await
        .unwrap();
    assert_eq!(result.result, 123);
    assert_metadata(&result);
    {
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0]["method"], "EXPERIMENTAL_call_function");
        assert_eq!(requests[0]["params"]["account_id"], "counter.testnet");
        assert_eq!(requests[0]["params"]["method_name"], "get_count");
        assert_eq!(requests[0]["params"]["args_base64"], "e30=");
        assert_eq!(
            requests[0]["params"]["block_id"],
            CryptoHash::from_bytes([7; 32]).to_string()
        );
    }
    let plain: u64 = near.view("counter.testnet", "get_count").await.unwrap();
    assert_eq!(plain, result.result);
}

#[tokio::test]
async fn borsh_value_preserves_metadata_and_plain_await() {
    let (near, requests) = client(borsh::to_vec(&123_u64).unwrap());
    let result = near
        .view::<u64>("counter.testnet", "get_count")
        .at_block(42)
        .borsh()
        .with_metadata()
        .await
        .unwrap();
    assert_eq!(result.result, 123);
    assert_metadata(&result);
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_eq!(requests.lock().unwrap()[0]["params"]["block_id"], 42);
    let plain: u64 = near
        .view("counter.testnet", "get_count")
        .borsh()
        .await
        .unwrap();
    assert_eq!(plain, result.result);
}

#[tokio::test]
async fn empty_json_response_still_decodes_as_null() {
    let (near, _) = client(Vec::new());
    let unit: ViewFunctionResult<()> = near
        .view("counter.testnet", "empty")
        .with_metadata()
        .await
        .unwrap();
    assert_metadata(&unit);
    let optional: ViewFunctionResult<Option<u64>> = near
        .view("counter.testnet", "empty")
        .with_metadata()
        .await
        .unwrap();
    assert_eq!(optional.result, None);
}

#[tokio::test]
async fn decoding_errors_keep_their_json_and_borsh_variants() {
    let (near, _) = client(vec![255]);
    assert!(matches!(
        near.view::<u64>("counter.testnet", "invalid")
            .with_metadata()
            .await,
        Err(Error::Json(_))
    ));
    assert!(matches!(
        near.view::<u64>("counter.testnet", "invalid")
            .borsh()
            .with_metadata()
            .await,
        Err(Error::Borsh(_))
    ));
}

struct InvalidArgs;

impl serde::Serialize for InvalidArgs {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("invalid arguments"))
    }
}

#[tokio::test]
async fn invalid_arguments_fail_before_sending_a_request() {
    let (near, requests) = client(b"123".to_vec());
    assert!(matches!(
        near.view::<u64>("counter.testnet", "get_count")
            .args(InvalidArgs)
            .with_metadata()
            .await,
        Err(Error::Json(_))
    ));
    assert!(requests.lock().unwrap().is_empty());
}

#[cfg(feature = "contracts")]
#[tokio::test]
async fn generated_contract_views_expose_metadata_without_macro_changes() {
    #[near_kit::contract]
    trait Counter {
        fn get_count(&self) -> u64;
    }

    let (near, _) = client(b"123".to_vec());
    let counter = near.contract::<Counter>("counter.testnet").unwrap();
    let result = counter.get_count().with_metadata().await.unwrap();
    assert_eq!(result.result, 123);
    assert_metadata(&result);
}
