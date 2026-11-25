//! near-kit against JSON and vectors that nearcore 2.15's own
//! `near-primitives` produced (`fixtures/nearcore_2_15.json`, written and
//! checked by the `nearcore-wire` crate next to this file).

use near_kit_primitives::protocol::{
    ActionErrorKind, ExternalCall, HostError, InvalidTxError, RawStateInit, TxExecutionError,
};
use near_kit_primitives::rpc::{
    AuthorizationContext, ExternalCallResponse, FinalExecutionOutcome, Receipt, ReceiptContent,
    TransactionEnvelopeView, TxExecutionStatus,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/nearcore_2_15.json")).unwrap()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn external_call_vectors() {
    let fixture = fixture();
    let vectors = fixture["external_calls"].as_array().unwrap();
    assert!(vectors.len() >= 9);
    for vector in vectors {
        let name = vector["name"].as_str().unwrap();
        let mut call = ExternalCall::new(
            vector["contract_id"]
                .as_str()
                .unwrap()
                .parse::<near_kit_primitives::AccountId>()
                .unwrap(),
            unhex(vector["payload"].as_str().unwrap()),
        );
        if let Some(state_init) = vector["state_init"].as_str() {
            call = call.with_state_init(RawStateInit(unhex(state_init)));
        }
        let bytes = unhex(vector["bytes"].as_str().unwrap());
        assert_eq!(call.to_bytes(), bytes, "{name}");
        assert_eq!(call.to_base64(), vector["base64"], "{name}");
        assert_eq!(ExternalCall::from_bytes(&bytes).unwrap(), call, "{name}");
        assert_eq!(call.call_hash().to_string(), vector["call_hash"], "{name}");
        for id in vector["unique_ids"].as_array().unwrap() {
            let height: u64 = id["height"].as_str().unwrap().parse().unwrap();
            assert_eq!(
                call.unique_id(height).to_string(),
                id["unique_id"],
                "{name} at {height}"
            );
        }
    }
}

#[test]
fn transaction_views_of_both_kinds() {
    let fixture = fixture();
    let views = &fixture["views"];
    for name in ["transaction signed", "transaction signed without kind"] {
        let view: TransactionEnvelopeView = serde_json::from_value(views[name].clone()).unwrap();
        let tx = view.as_signed().unwrap_or_else(|| panic!("{name}"));
        assert_eq!(view.id(), tx.hash);
        assert_eq!(view.sender_id(), &tx.signer_id);
        assert_eq!(view.receiver_id().as_str(), "bob.near");
    }
    let call = ExternalCall::new(
        "wallet.near"
            .parse::<near_kit_primitives::AccountId>()
            .unwrap(),
        vec![1, 2, 3],
    );
    for (name, state_init) in [
        ("transaction external call", None),
        (
            "transaction external call with state init",
            Some(vec![4, 5, 6]),
        ),
    ] {
        let view: TransactionEnvelopeView = serde_json::from_value(views[name].clone()).unwrap();
        let ecc = view.as_external_call().unwrap_or_else(|| panic!("{name}"));
        assert_eq!(ecc.payload, [1, 2, 3]);
        assert_eq!(ecc.state_init, state_init);
        assert_eq!(view.id(), call.unique_id(1));
        assert_eq!(view.sender_id().as_str(), "wallet.near");
        assert_eq!(view.receiver_id().as_str(), "wallet.near");
        assert!(view.as_signed().is_none());
    }
}

#[test]
fn transaction_view_kind_errors() {
    let fixture = fixture();
    let signed = &fixture["views"]["transaction signed"];
    let with_kind = |kind: Value| {
        let mut json = signed.clone();
        json["kind"] = kind;
        serde_json::from_value::<TransactionEnvelopeView>(json)
            .unwrap_err()
            .to_string()
    };
    let err = with_kind("delegate".into());
    assert!(err.contains("unknown variant `delegate`"), "{err}");
    let err = with_kind(1.into());
    assert!(err.contains("`kind` to be a string"), "{err}");
    // An external call's kind on a signed transaction's fields keeps the
    // field error instead of a generic "no variant matched".
    let err = with_kind("external_call".into());
    assert!(err.contains("contract_id"), "{err}");
    let err = serde_json::from_value::<TransactionEnvelopeView>(Value::from(1)).unwrap_err();
    assert!(err.to_string().contains("object"), "{err}");
}

#[test]
fn external_call_outcomes() {
    let fixture = fixture();
    let views = &fixture["views"];
    let call = ExternalCall::new(
        "wallet.near"
            .parse::<near_kit_primitives::AccountId>()
            .unwrap(),
        vec![1, 2, 3],
    );

    let accepted: FinalExecutionOutcome =
        serde_json::from_value(views["outcome external call accepted"].clone()).unwrap();
    assert_eq!(accepted.transaction_hash(), &call.unique_id(1));

    // A send's answer names the copy once there is one.
    let mut answer = views["outcome external call accepted"].clone();
    answer["final_execution_status"] = "FINAL".into();
    let answer: ExternalCallResponse = serde_json::from_value(answer).unwrap();
    assert_eq!(answer.final_execution_status, TxExecutionStatus::Final);
    assert_eq!(answer.transaction_id(), Some(call.unique_id(1)));
    let before: ExternalCallResponse =
        serde_json::from_value(serde_json::json!({ "final_execution_status": "NONE" })).unwrap();
    assert_eq!(before.final_execution_status, TxExecutionStatus::None);
    assert_eq!(before.transaction_id(), None);
    assert!(before.outcome.is_none());
    assert_eq!(accepted.json::<String>().unwrap(), "ok");
    assert_eq!(accepted.logs().collect::<Vec<_>>(), ["accepted"]);

    let refused: FinalExecutionOutcome =
        serde_json::from_value(views["outcome external call refused in the run"].clone()).unwrap();
    assert!(refused.is_failure());
    assert_eq!(refused.logs().collect::<Vec<_>>(), ["checking"]);
    assert!(
        refused
            .result()
            .unwrap_err()
            .to_string()
            .contains("refused")
    );

    let not_found: FinalExecutionOutcome =
        serde_json::from_value(views["outcome external call refused at execution"].clone())
            .unwrap();
    assert!(matches!(
        not_found.failure_error(),
        Some(TxExecutionError::InvalidTxError(
            InvalidTxError::ContractNotFound { .. }
        ))
    ));
}

#[test]
fn action_receipt_v3_views() {
    let fixture = fixture();
    let views = &fixture["views"];
    let decode = |name: &str| {
        let receipt: Receipt = serde_json::from_value(views[name].clone()).unwrap();
        match receipt.receipt {
            ReceiptContent::Action(action) => action,
            other => panic!("{name}: {other:?}"),
        }
    };
    let signed = decode("receipt v3 access key");
    let key = signed
        .signer_public_key
        .clone()
        .expect("a signed chain names its key");
    assert_eq!(
        signed.authorization_context,
        Some(AuthorizationContext::AccessKey(key))
    );
    assert_eq!(signed.creation_protocol_version, Some(89));

    let ecc = decode("receipt v3 external call");
    assert_eq!(ecc.signer_public_key, None);
    assert_eq!(
        ecc.authorization_context,
        Some(AuthorizationContext::ExternalContractCall)
    );
    assert_eq!(ecc.creation_protocol_version, Some(89));
}

#[test]
fn new_errors_decode_to_their_variants() {
    let fixture = fixture();
    for (name, value) in fixture["errors"].as_object().unwrap() {
        let text = if name.starts_with("HostError") {
            let error: HostError = serde_json::from_value(value.clone()).unwrap();
            assert!(!matches!(error, HostError::Unknown(_)), "{name}");
            error.to_string()
        } else {
            let error: TxExecutionError = serde_json::from_value(value.clone()).unwrap();
            let unknown = match &error {
                TxExecutionError::InvalidTxError(e) => matches!(e, InvalidTxError::Unknown(_)),
                TxExecutionError::ActionError(e) => matches!(e.kind, ActionErrorKind::Unknown(_)),
                _ => true,
            };
            assert!(!unknown, "{name}: {error:?}");
            error.to_string()
        };
        assert!(!text.contains("unknown"), "{name}: {text}");
    }
}
