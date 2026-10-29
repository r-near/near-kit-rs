//! JSON-RPC response bodies for the client's unit tests.

use serde_json::{Value, json};

/// A successful response carrying `result`.
pub(crate) fn rpc_result(result: Value) -> Vec<u8> {
    envelope(json!({ "result": result }))
}

/// A `HANDLER_ERROR` response as nearcore sends it. `cause: None` leaves the
/// cause out, as older nodes and proxies do.
pub(crate) fn rpc_error(cause: Option<&str>, info: Value, data: Value) -> Vec<u8> {
    let mut error = json!({
        "name": "HANDLER_ERROR",
        "code": -32000,
        "message": "Server error",
        "data": data,
    });
    if let Some(cause) = cause {
        error["cause"] = json!({ "name": cause, "info": info });
    }
    envelope(json!({ "error": error }))
}

/// A `send_tx` rejected with the `InvalidTxError` `error`.
pub(crate) fn invalid_tx_body(error: Value) -> Vec<u8> {
    rpc_error(
        Some("INVALID_TRANSACTION"),
        json!({}),
        json!({ "TxExecutionError": { "InvalidTxError": error } }),
    )
}

fn envelope(mut response: Value) -> Vec<u8> {
    response["jsonrpc"] = "2.0".into();
    response["id"] = 0.into();
    serde_json::to_vec(&response).unwrap()
}
