//! near-kit against nearcore 2.15's `near-primitives`, for external calls
//! and the views and errors 2.15 changed.

use std::path::PathBuf;

use borsh::BorshDeserialize;
use near_kit_primitives::protocol::{
    ActionErrorKind as KitActionErrorKind, ActionsValidationError as KitActionsValidationError,
    ExternalCall, ExternalCallChunkLimit as KitChunkLimit, HostError as KitHostError,
    InvalidTxError as KitInvalidTxError, RawStateInit as KitRawStateInit,
    SignedTransaction as KitSignedTransaction, StorageError as KitStorageError, Transaction,
    TxExecutionError as KitTxExecutionError, UniversalStateInit, UniversalStateInitV1,
};
use near_kit_primitives::rpc::{
    AuthorizationContext as KitAuthorizationContext, FinalExecutionOutcome, Receipt as KitReceipt,
    ReceiptContent, TransactionEnvelopeView as KitEnvelopeView,
};
use near_kit_primitives::signer::SecretKey;
use near_kit_primitives::{AccountId as KitAccountId, CryptoHash as KitHash, NearToken};
use near_primitives::errors::{
    ActionError, ActionErrorKind, ActionsValidationError, ExternalCallChunkLimit, HostError,
    InvalidTxError, StorageError, TxExecutionError,
};
use near_primitives::hash::CryptoHash;
use near_primitives::receipt::{
    ActionReceiptV3, AuthorizationContext, CreationProtocolVersion, Receipt, ReceiptEnum, ReceiptV0,
};
use near_primitives::transaction::{
    Action, EccTransaction, ExecutionOutcome, ExecutionOutcomeWithId,
    ExecutionOutcomeWithIdAndProof, ExecutionStatus, SignedTransaction, TransactionEnvelope,
    TransferAction,
};
use near_primitives::types::{AccountId, Balance};
use near_primitives::universal_state_init::RawStateInit;
use near_primitives::utils::create_ecc_outcome_id;
use near_primitives::views::{
    EccTransactionView, FinalExecutionOutcomeView, FinalExecutionStatus, ReceiptView,
    SignedTransactionView, TransactionEnvelopeView,
};
use serde_json::{Value, json};

const HEIGHTS: [u64; 6] = [0, 1, 7, 8, 1 << 32, u64::MAX];

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn nearcore_ecc(call: &ExternalCall) -> EccTransaction {
    EccTransaction::new_with_state_init(
        call.contract_id().as_str().parse().unwrap(),
        call.payload().to_vec(),
        call.state_init().map(|s| RawStateInit(s.0.clone())),
    )
}

fn kit_hash(hash: &CryptoHash) -> KitHash {
    KitHash::from_bytes(hash.0)
}

/// A universal account's own state init, holding one access key.
fn universal_call() -> ExternalCall {
    let key = SecretKey::ed25519_from_bytes([7; 32]);
    let state_init: UniversalStateInit = UniversalStateInitV1::default()
        .with_access_key(key.public_key())
        .into();
    let account_id = state_init.derive_account_id();
    ExternalCall::new(account_id, vec![0xAB; 4]).with_state_init(state_init)
}

/// The vector inputs: nearcore's own test calls, then edge cases.
fn calls() -> Vec<(&'static str, ExternalCall)> {
    let wallet: KitAccountId = "wallet.near".parse().unwrap();
    let longest: KitAccountId = "a".repeat(64).parse().unwrap();
    let implicit: KitAccountId = "0".repeat(64).parse().unwrap();
    vec![
        ("nearcore", ExternalCall::new(wallet.clone(), vec![1, 2, 3])),
        (
            "nearcore with state init",
            ExternalCall::new(wallet.clone(), vec![1, 2, 3])
                .with_state_init(KitRawStateInit(vec![4, 5, 6])),
        ),
        (
            "contract.near",
            ExternalCall::new(
                "contract.near".parse::<KitAccountId>().unwrap(),
                vec![1, 2, 3],
            ),
        ),
        (
            "empty payload",
            ExternalCall::new(wallet.clone(), Vec::<u8>::new()),
        ),
        (
            "empty state init",
            ExternalCall::new(wallet.clone(), vec![1]).with_state_init(KitRawStateInit(vec![])),
        ),
        (
            "300-byte payload",
            ExternalCall::new(wallet, (0..300u32).map(|i| i as u8).collect::<Vec<_>>()),
        ),
        (
            "64-character account",
            ExternalCall::new(longest, b"x".to_vec()),
        ),
        (
            "implicit account",
            ExternalCall::new(implicit, b"{}".to_vec()),
        ),
        ("universal account with its state init", universal_call()),
    ]
}

/// xorshift64*, so the sweep is the same on every run.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn bytes(&mut self, max_len: u64) -> Vec<u8> {
        let len = self.next() % (max_len + 1);
        (0..len).map(|_| self.next() as u8).collect()
    }
}

fn assert_matches_nearcore(name: &str, call: &ExternalCall) {
    let ecc = nearcore_ecc(call);
    let bytes = borsh::to_vec(&ecc).unwrap();
    assert_eq!(call.to_bytes(), bytes, "{name}: encoding");
    assert_eq!(
        borsh::to_vec(&TransactionEnvelope::from(ecc.clone())).unwrap(),
        bytes,
        "{name}: envelope encoding"
    );
    assert_eq!(
        ExternalCall::from_bytes(&bytes).unwrap(),
        *call,
        "{name}: decode"
    );
    assert_eq!(
        EccTransaction::try_from_slice(&call.to_bytes()).unwrap(),
        ecc,
        "{name}: nearcore decodes near-kit's bytes"
    );
    assert_eq!(
        call.call_hash(),
        kit_hash(ecc.hash_without_state_init()),
        "{name}: call hash"
    );
    for height in HEIGHTS {
        assert_eq!(
            call.unique_id(height),
            kit_hash(&create_ecc_outcome_id(
                ecc.hash_without_state_init(),
                height
            )),
            "{name}: unique id at {height}"
        );
    }
}

#[test]
fn external_calls_match_nearcore() {
    for (name, call) in calls() {
        assert_matches_nearcore(name, &call);
    }
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for i in 0..2000 {
        let len = 2 + rng.next() % 63;
        let account: String = (0..len)
            .map(|_| (b'a' + (rng.next() % 26) as u8) as char)
            .collect();
        let mut call = ExternalCall::new(account.parse::<KitAccountId>().unwrap(), rng.bytes(600));
        if rng.next() % 2 == 0 {
            call = call.with_state_init(KitRawStateInit(rng.bytes(200)));
        }
        assert_matches_nearcore(&format!("random call {i}"), &call);
    }
}

#[test]
fn near_kit_rejects_what_nearcore_rejects() {
    let call = ExternalCall::new(
        "wallet.near".parse::<KitAccountId>().unwrap(),
        vec![1, 2, 3],
    );
    let mut cases = Vec::new();
    for (index, byte) in [(0, 0xFE), (1, 1)] {
        let mut bytes = call.to_bytes();
        bytes[index] = byte;
        cases.push(bytes);
    }
    let mut trailing = call.to_bytes();
    trailing.push(0);
    cases.push(trailing);
    let mut truncated = call.to_bytes();
    truncated.pop();
    cases.push(truncated);
    for bytes in cases {
        assert!(EccTransaction::try_from_slice(&bytes).is_err(), "{bytes:?}");
        assert!(ExternalCall::from_bytes(&bytes).is_err(), "{bytes:?}");
    }
}

/// Decode `bytes` with both, and require the same verdict; when both
/// accept, both must read the same call and write the same bytes back.
fn assert_same_verdict(name: &str, bytes: &[u8]) -> bool {
    let nearcore = EccTransaction::try_from_slice(bytes);
    let kit = ExternalCall::from_bytes(bytes);
    assert_eq!(
        kit.is_ok(),
        nearcore.is_ok(),
        "{name}: {} decodes {}: near-kit {kit:?}, nearcore {:?}",
        hex(bytes),
        if nearcore.is_ok() {
            "in nearcore"
        } else {
            "in near-kit"
        },
        nearcore.as_ref().err()
    );
    if let (Ok(kit), Ok(nearcore)) = (kit, nearcore) {
        assert_eq!(kit.to_bytes(), bytes, "{name}: near-kit writes it back");
        assert_eq!(borsh::to_vec(&nearcore).unwrap(), bytes, "{name}");
        assert_eq!(nearcore_ecc(&kit), nearcore, "{name}: the same call");
        return true;
    }
    false
}

/// An encoding with `account` written as the contract's bytes, valid or not.
fn encoding_with_account(account: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0x00];
    bytes.extend_from_slice(&(account.len() as u32).to_le_bytes());
    bytes.extend_from_slice(account);
    bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes.push(0);
    bytes
}

#[test]
fn near_kit_decodes_exactly_what_nearcore_decodes() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut accepted = 0;
    let mut total = 0;
    let mut check = |name: &str, bytes: &[u8]| {
        total += 1;
        accepted += usize::from(assert_same_verdict(name, bytes));
    };

    // Account ids at the edges of validity, valid and not.
    let long = |n: usize| "a".repeat(n);
    let mut accounts: Vec<Vec<u8>> = [
        "", "a", "ab", "a.b", "a-b", "a_b", "0", "00", ".a", "a.", "-a", "a-", "_a", "a_", "a..b",
        "a--b", "a__b", "a.-b", "A", "aB", "a b", "a@b", "a/b", "a+b", "é", "a\0", "near",
        "system", "0u", "0s", "0x", "deadbeef",
    ]
    .iter()
    .map(|a| a.as_bytes().to_vec())
    .collect();
    for n in [2, 63, 64, 65, 100] {
        accounts.push(long(n).into_bytes());
    }
    accounts.push(format!("0u{}", "a".repeat(40)).into_bytes());
    accounts.push(format!("0s{}", "0".repeat(40)).into_bytes());
    accounts.push("0".repeat(64).into_bytes());
    accounts.push("f".repeat(63).into_bytes());
    accounts.push(vec![0xFF, 0xFE]);
    const ALPHABET: &[u8] = b"abz09._-A@ ";
    for _ in 0..2000 {
        let len = rng.next() % 70;
        accounts.push(
            (0..len)
                .map(|_| ALPHABET[(rng.next() % ALPHABET.len() as u64) as usize])
                .collect(),
        );
    }
    for account in &accounts {
        check("account id", &encoding_with_account(account, b"{}"));
    }

    // Mutations of valid encodings.
    let mut seeds: Vec<Vec<u8>> = calls().iter().map(|(_, call)| call.to_bytes()).collect();
    for _ in 0..200 {
        let mut call = ExternalCall::new(
            "wallet.near".parse::<KitAccountId>().unwrap(),
            rng.bytes(40),
        );
        if rng.next() % 2 == 0 {
            call = call.with_state_init(KitRawStateInit(rng.bytes(40)));
        }
        seeds.push(call.to_bytes());
    }
    for seed in &seeds {
        check("seed", seed);
        for _ in 0..40 {
            let mut bytes = seed.clone();
            let at = (rng.next() % bytes.len() as u64) as usize;
            match rng.next() % 8 {
                0 => bytes[at] ^= 1 << (rng.next() % 8),
                1 => bytes[at] = rng.next() as u8,
                2 => bytes.truncate(at),
                3 => bytes.extend(rng.bytes(8).into_iter().chain([0])),
                4 => bytes.insert(at, rng.next() as u8),
                5 => {
                    bytes.remove(at);
                }
                // The option tag, which only 0 and 1 are.
                6 => *bytes.last_mut().unwrap() = 2 + (rng.next() % 254) as u8,
                // A length prefix running past the end, or shorter than the
                // bytes that follow.
                _ => {
                    let len = match rng.next() % 3 {
                        0 => u32::MAX,
                        1 => bytes.len() as u32,
                        _ => (rng.next() % 4) as u32,
                    };
                    bytes[2..6].copy_from_slice(&len.to_le_bytes());
                }
            }
            check("mutation", &bytes);
        }
    }
    // A state init's tag past the end and with no length.
    let call = ExternalCall::new("wallet.near".parse::<KitAccountId>().unwrap(), vec![1]);
    for tail in [
        &[1u8][..],
        &[1, 0, 0, 0],
        &[1, 255, 255, 255, 255],
        &[2],
        &[255],
    ] {
        let mut bytes = call.to_bytes();
        bytes.pop();
        bytes.extend_from_slice(tail);
        check("state init tag", &bytes);
    }

    // Both kinds of verdict are well represented.
    eprintln!("{accepted} of {total} encodings decode");
    assert!(total > 10_000, "{total}");
    assert!(
        accepted > 500 && total - accepted > 1000,
        "{accepted} of {total}"
    );
}

fn signed_transaction() -> (KitSignedTransaction, SignedTransaction) {
    let key = SecretKey::ed25519_from_bytes([1; 32]);
    let kit = Transaction::new(
        "alice.near".parse::<KitAccountId>().unwrap(),
        key.public_key(),
        1,
        "bob.near".parse::<KitAccountId>().unwrap(),
        KitHash::ZERO,
        vec![near_kit_primitives::protocol::Action::transfer(
            NearToken::from_yoctonear(1),
        )],
    )
    .sign(&key);
    let nearcore = SignedTransaction::try_from_slice(&kit.to_bytes()).unwrap();
    (kit, nearcore)
}

fn external_call_view(call: &ExternalCall, height: u64) -> TransactionEnvelopeView {
    let ecc = nearcore_ecc(call);
    let id = create_ecc_outcome_id(ecc.hash_without_state_init(), height);
    TransactionEnvelopeView::Unsigned(EccTransactionView::new(ecc, id))
}

fn outcome_view(
    transaction: TransactionEnvelopeView,
    executor_id: &str,
    status: ExecutionStatus,
    logs: Vec<String>,
) -> FinalExecutionOutcomeView {
    let final_status = match &status {
        ExecutionStatus::SuccessValue(value) => FinalExecutionStatus::SuccessValue(value.clone()),
        ExecutionStatus::Failure(error) => FinalExecutionStatus::Failure(error.clone()),
        _ => FinalExecutionStatus::NotStarted,
    };
    let outcome = ExecutionOutcomeWithIdAndProof {
        proof: vec![],
        block_hash: CryptoHash::hash_bytes(b"block"),
        outcome_with_id: ExecutionOutcomeWithId {
            id: transaction.id(),
            outcome: ExecutionOutcome {
                logs,
                executor_id: executor_id.parse().unwrap(),
                status,
                ..Default::default()
            },
        },
    };
    FinalExecutionOutcomeView {
        status: final_status,
        transaction,
        transaction_outcome: outcome.into(),
        receipts_outcome: vec![],
    }
}

fn receipt_view(authorization_context: AuthorizationContext) -> ReceiptView {
    let action_receipt = ActionReceiptV3 {
        signer_id: "wallet.near".parse().unwrap(),
        refund_to: None,
        authorization_context,
        gas_price: Balance::from_yoctonear(100_000_000),
        creation_protocol_version: CreationProtocolVersion(89),
        output_data_receivers: vec![],
        input_data_ids: vec![],
        actions: vec![Action::Transfer(TransferAction {
            deposit: Balance::from_yoctonear(1),
        })],
    };
    Receipt::V0(ReceiptV0 {
        predecessor_id: "wallet.near".parse().unwrap(),
        receiver_id: "wallet.near".parse().unwrap(),
        receipt_id: CryptoHash::hash_bytes(b"receipt"),
        receipt: ReceiptEnum::ActionV3(action_receipt),
    })
    .into()
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

/// nearcore's JSON for every view and error 2.15 changed, by name.
fn views() -> Vec<(&'static str, Value)> {
    let (_, signed) = signed_transaction();
    let signed_view = TransactionEnvelopeView::Signed(SignedTransactionView::from(signed));
    let mut without_kind = to_json(&signed_view);
    without_kind.as_object_mut().unwrap().remove("kind");
    let call = ExternalCall::new(
        "wallet.near".parse::<KitAccountId>().unwrap(),
        vec![1, 2, 3],
    );
    let with_state_init = call.clone().with_state_init(KitRawStateInit(vec![4, 5, 6]));
    let key: near_crypto::PublicKey = SecretKey::ed25519_from_bytes([1; 32])
        .public_key()
        .to_string()
        .parse()
        .unwrap();
    let refused = TxExecutionError::ActionError(ActionError {
        index: Some(0),
        kind: ActionErrorKind::FunctionCallError(
            near_primitives::errors::FunctionCallError::ExecutionError(
                "Smart contract panicked: refused".to_owned(),
            ),
        ),
    });
    vec![
        ("transaction signed", to_json(&signed_view)),
        ("transaction signed without kind", without_kind),
        (
            "transaction external call",
            to_json(&external_call_view(&call, 1)),
        ),
        (
            "transaction external call with state init",
            to_json(&external_call_view(&with_state_init, 1)),
        ),
        (
            "outcome external call accepted",
            to_json(&outcome_view(
                external_call_view(&call, 1),
                "wallet.near",
                ExecutionStatus::SuccessValue(b"\"ok\"".to_vec()),
                vec!["accepted".to_owned()],
            )),
        ),
        (
            "outcome external call refused in the run",
            to_json(&outcome_view(
                external_call_view(&call, 1),
                "wallet.near",
                ExecutionStatus::Failure(refused),
                vec!["checking".to_owned()],
            )),
        ),
        (
            "outcome external call refused at execution",
            to_json(&outcome_view(
                external_call_view(&call, 1),
                "wallet.near",
                ExecutionStatus::Failure(TxExecutionError::InvalidTxError(
                    InvalidTxError::ContractNotFound {
                        contract_id: "wallet.near".parse().unwrap(),
                    },
                )),
                vec![],
            )),
        ),
        (
            "receipt v3 access key",
            to_json(&receipt_view(AuthorizationContext::AccessKey(key))),
        ),
        (
            "receipt v3 external call",
            to_json(&receipt_view(AuthorizationContext::ExternalContractCall)),
        ),
    ]
}

fn errors() -> Vec<(&'static str, Value)> {
    let wallet: AccountId = "wallet.near".parse().unwrap();
    let action = |kind| {
        TxExecutionError::ActionError(ActionError {
            index: Some(0),
            kind,
        })
    };
    let tx = TxExecutionError::InvalidTxError;
    vec![
        (
            "ContractNotFound",
            to_json(&tx(InvalidTxError::ContractNotFound {
                contract_id: wallet,
            })),
        ),
        (
            "ExternalCallChunkShareReached Compute",
            to_json(&tx(InvalidTxError::ExternalCallChunkShareReached {
                share: ExternalCallChunkLimit::Compute {
                    used: 400,
                    limit: 300,
                },
            })),
        ),
        (
            "ExternalCallChunkShareReached StorageProof",
            to_json(&tx(InvalidTxError::ExternalCallChunkShareReached {
                share: ExternalCallChunkLimit::StorageProof { used: 5, limit: 4 },
            })),
        ),
        (
            "RecordingBudgetExceeded",
            to_json(&tx(InvalidTxError::StorageError(
                StorageError::RecordingBudgetExceeded { limit: 51200 },
            ))),
        ),
        (
            "ReservedMethodName",
            to_json(&action(ActionErrorKind::ReservedMethodName {
                method_name: "__external_call".to_owned(),
            })),
        ),
        (
            "TotalNumberOfGasKeyNoncesRemovedExceeded",
            to_json(&action(
                ActionErrorKind::TotalNumberOfGasKeyNoncesRemovedExceeded {
                    number_of_nonces: 1025,
                    limit: 1024,
                },
            )),
        ),
        (
            "TotalNumberOfGasKeyNoncesAddedExceeded",
            to_json(&tx(InvalidTxError::ActionsValidation(
                ActionsValidationError::TotalNumberOfGasKeyNoncesAddedExceeded {
                    number_of_nonces: 1025,
                    limit: 1024,
                },
            ))),
        ),
        (
            "HostError ProhibitedInExternalCall",
            to_json(&HostError::ProhibitedInExternalCall {
                method_name: "random_seed".to_owned(),
            }),
        ),
        (
            "HostError DepositInExternalCall",
            to_json(&HostError::DepositInExternalCall),
        ),
    ]
}

fn call_vector(name: &str, call: &ExternalCall) -> Value {
    let ecc = nearcore_ecc(call);
    let bytes = borsh::to_vec(&ecc).unwrap();
    json!({
        "name": name,
        "contract_id": call.contract_id().as_str(),
        "payload": hex(call.payload()),
        "state_init": call.state_init().map(|s| hex(&s.0)),
        "bytes": hex(&bytes),
        "base64": call.to_base64(),
        "call_hash": ecc.hash_without_state_init().to_string(),
        "tx_root_leaf_hash": ecc.hash().to_string(),
        "unique_ids": HEIGHTS
            .iter()
            .map(|&height| json!({
                "height": height.to_string(),
                "unique_id": create_ecc_outcome_id(ecc.hash_without_state_init(), height).to_string(),
            }))
            .collect::<Vec<_>>(),
    })
}

fn fixture() -> Value {
    let named = |entries: Vec<(&str, Value)>| {
        entries
            .into_iter()
            .map(|(name, value)| (name.to_owned(), value))
            .collect()
    };
    json!({
        "source": "near-primitives 0.39.0-rc.1 (nearcore 2.15.0-rc.1)",
        "external_calls": calls().iter().map(|(name, call)| call_vector(name, call)).collect::<Vec<_>>(),
        "views": Value::Object(named(views())),
        "errors": Value::Object(named(errors())),
    })
}

#[test]
fn fixture_is_what_nearcore_produces() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/nearcore_2_15.json");
    let expected = serde_json::to_string_pretty(&fixture()).unwrap() + "\n";
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        actual == expected,
        "{} is stale: rerun with BLESS=1",
        path.display()
    );
}

#[test]
fn near_kit_decodes_nearcore_views() {
    let views: serde_json::Map<String, Value> = views()
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect();
    let (kit_signed, _) = signed_transaction();
    for name in ["transaction signed", "transaction signed without kind"] {
        let view: KitEnvelopeView = serde_json::from_value(views[name].clone()).unwrap();
        let tx = view
            .as_signed()
            .unwrap_or_else(|| panic!("{name}: {view:?}"));
        assert_eq!(tx.hash, kit_signed.get_hash(), "{name}");
        assert_eq!(view.id(), kit_signed.get_hash(), "{name}");
        assert_eq!(view.sender_id().as_str(), "alice.near", "{name}");
    }
    let call = ExternalCall::new(
        "wallet.near".parse::<KitAccountId>().unwrap(),
        vec![1, 2, 3],
    );
    for (name, state_init) in [
        ("transaction external call", None),
        (
            "transaction external call with state init",
            Some(vec![4, 5, 6]),
        ),
    ] {
        let view: KitEnvelopeView = serde_json::from_value(views[name].clone()).unwrap();
        let ecc = view
            .as_external_call()
            .unwrap_or_else(|| panic!("{name}: {view:?}"));
        assert_eq!(ecc.contract_id.as_str(), "wallet.near");
        assert_eq!(ecc.payload, [1, 2, 3]);
        assert_eq!(ecc.state_init, state_init, "{name}");
        assert_eq!(ecc.transaction_id, call.unique_id(1), "{name}");
        assert_eq!(view.id(), call.unique_id(1));
        assert_eq!(view.sender_id().as_str(), "wallet.near");
    }

    let accepted: FinalExecutionOutcome =
        serde_json::from_value(views["outcome external call accepted"].clone()).unwrap();
    assert_eq!(accepted.transaction_hash(), &call.unique_id(1));
    assert_eq!(accepted.json::<String>().unwrap(), "ok");
    assert_eq!(accepted.logs().collect::<Vec<_>>(), ["accepted"]);

    let refused: FinalExecutionOutcome =
        serde_json::from_value(views["outcome external call refused in the run"].clone()).unwrap();
    assert!(refused.is_failure());
    assert_eq!(refused.logs().collect::<Vec<_>>(), ["checking"]);
    let error = refused.result().unwrap_err().to_string();
    assert!(
        error.contains("Smart contract panicked: refused"),
        "{error}"
    );

    let not_found: FinalExecutionOutcome =
        serde_json::from_value(views["outcome external call refused at execution"].clone())
            .unwrap();
    assert!(matches!(
        not_found.failure_error(),
        Some(KitTxExecutionError::InvalidTxError(
            KitInvalidTxError::ContractNotFound { .. }
        ))
    ));

    for (name, expected) in [
        (
            "receipt v3 access key",
            Some(KitAuthorizationContext::AccessKey(
                SecretKey::ed25519_from_bytes([1; 32]).public_key(),
            )),
        ),
        (
            "receipt v3 external call",
            Some(KitAuthorizationContext::ExternalContractCall),
        ),
    ] {
        let receipt: KitReceipt = serde_json::from_value(views[name].clone()).unwrap();
        let ReceiptContent::Action(action) = &receipt.receipt else {
            panic!("{name}: {receipt:?}")
        };
        assert_eq!(action.authorization_context, expected, "{name}");
        assert_eq!(action.creation_protocol_version, Some(89), "{name}");
        let signer_public_key = match &expected {
            Some(KitAuthorizationContext::AccessKey(key)) => Some(key.clone()),
            _ => None,
        };
        assert_eq!(action.signer_public_key, signer_public_key, "{name}");
    }
}

#[test]
fn near_kit_decodes_nearcore_errors() {
    for (name, value) in errors() {
        let ok = if name.starts_with("HostError") {
            let error: KitHostError = serde_json::from_value(value.clone()).unwrap();
            matches!(
                error,
                KitHostError::ProhibitedInExternalCall { .. } | KitHostError::DepositInExternalCall
            )
        } else {
            let error: KitTxExecutionError = serde_json::from_value(value.clone()).unwrap();
            match error {
                KitTxExecutionError::InvalidTxError(error) => matches!(
                    error,
                    KitInvalidTxError::ContractNotFound { .. }
                        | KitInvalidTxError::ExternalCallChunkShareReached {
                            share: KitChunkLimit::Compute { .. }
                                | KitChunkLimit::StorageProof { .. },
                            ..
                        }
                        | KitInvalidTxError::StorageError(
                            KitStorageError::RecordingBudgetExceeded { .. }
                        )
                        | KitInvalidTxError::ActionsValidation(
                            KitActionsValidationError::TotalNumberOfGasKeyNoncesAddedExceeded { .. }
                        )
                ),
                KitTxExecutionError::ActionError(error) => matches!(
                    error.kind,
                    KitActionErrorKind::ReservedMethodName { .. }
                        | KitActionErrorKind::TotalNumberOfGasKeyNoncesRemovedExceeded { .. }
                ),
                _ => false,
            }
        };
        assert!(ok, "{name} decoded to the wrong variant: {value}");
    }
}
