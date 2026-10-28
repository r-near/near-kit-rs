//! `send_tx` / `EXPERIMENTAL_tx_status` responses for relayed (delegate)
//! transactions, captured from a nearcore 2.14.0 sandbox.
//!
//! nearcore renders a delegate's inner actions in the raw protocol `Action`
//! JSON, not the `ActionView` shape used for top-level actions.

use near_kit_primitives::protocol::{Action, DelegateAction, GlobalContractId, StateInit};
use near_kit_primitives::rpc::{
    ActionView, FinalExecutionOutcome, ReceiptContent, RpcTransactionResponse,
};
use near_kit_primitives::signer::PublicKey;
use near_kit_primitives::transaction::{Final, WaitLevel};
use near_kit_primitives::{AccountId, CryptoHash, NearToken};

const STATE_INIT: &str = include_str!("fixtures/delegate_state_init_send_tx.json");
const USE_GLOBAL_BY_HASH: &str = include_str!("fixtures/delegate_use_global_by_hash_send_tx.json");
const USE_GLOBAL_BY_ACCOUNT: &str =
    include_str!("fixtures/delegate_use_global_by_account_send_tx.json");
const CREATE_ACCOUNT: &str = include_str!("fixtures/delegate_create_account_send_tx.json");
const CREATE_ACCOUNT_TX_STATUS: &str =
    include_str!("fixtures/delegate_create_account_tx_status.json");
const TRANSFER: &str = include_str!("fixtures/delegate_transfer_send_tx.json");

const ALL: [(&str, &str); 6] = [
    ("state_init", STATE_INIT),
    ("use_global_by_hash", USE_GLOBAL_BY_HASH),
    ("use_global_by_account", USE_GLOBAL_BY_ACCOUNT),
    ("create_account", CREATE_ACCOUNT),
    ("create_account_tx_status", CREATE_ACCOUNT_TX_STATUS),
    ("transfer", TRANSFER),
];

fn sender() -> AccountId {
    "relayer.sandbox".parse().unwrap()
}

#[test]
fn relayed_outcomes_decode_directly() {
    for (name, body) in ALL {
        if let Err(e) = serde_json::from_str::<FinalExecutionOutcome>(body) {
            panic!("{name}: outcome failed to decode: {e}");
        }
    }
}

#[test]
fn relayed_transactions_resolve_at_final() {
    for (name, body) in ALL {
        let response: RpcTransactionResponse = serde_json::from_str(body).unwrap();
        match Final::convert(response, &sender()) {
            Ok(outcome) => assert!(outcome.is_success(), "{name}: {:?}", outcome.status),
            Err(e) => panic!("{name}: {e}"),
        }
    }
}

/// The inner actions of the single delegate action in `body`'s transaction.
fn inner_actions(body: &str) -> Vec<Action> {
    let outcome: FinalExecutionOutcome = serde_json::from_str(body).unwrap();
    match &outcome.transaction.actions[..] {
        [
            ActionView::Delegate {
                delegate_action, ..
            },
        ] => delegate_action.actions.clone(),
        other => panic!("expected one delegate action, got {other:?}"),
    }
}

fn hash(s: &str) -> CryptoHash {
    s.parse().unwrap()
}

#[test]
fn relayed_state_init_inner_action() {
    let [Action::DeterministicStateInit(init)] = &inner_actions(STATE_INIT)[..] else {
        panic!("expected one DeterministicStateInit");
    };
    assert_eq!(init.deposit, NearToken::from_near(1));
    let StateInit::V1(v1) = &init.state_init;
    assert_eq!(
        v1.code,
        GlobalContractId::from(hash("EmTQGUyEmTpd6BigmqBtYqZcX9ps8zJ48UPQHSc16vm9"))
    );
    assert_eq!(v1.data.get(&b"k"[..]).map(Vec::as_slice), Some(&b"v"[..]));
    assert_eq!(
        init.derive_account_id().as_str(),
        "0s4a67401515b4d005ce9848a96b5c36ed95e50ef3"
    );
}

#[test]
fn relayed_use_global_contract_inner_actions() {
    let [Action::UseGlobalContract(by_hash)] = &inner_actions(USE_GLOBAL_BY_HASH)[..] else {
        panic!("expected one UseGlobalContract");
    };
    assert_eq!(
        by_hash.contract_identifier,
        GlobalContractId::from(hash("EmTQGUyEmTpd6BigmqBtYqZcX9ps8zJ48UPQHSc16vm9"))
    );

    let [Action::UseGlobalContract(by_account)] = &inner_actions(USE_GLOBAL_BY_ACCOUNT)[..] else {
        panic!("expected one UseGlobalContract");
    };
    assert_eq!(
        by_account.contract_identifier,
        GlobalContractId::AccountId("sandbox".parse().unwrap())
    );
}

#[test]
fn relayed_create_account_inner_actions() {
    let full_access_key: PublicKey = "ed25519:A435yoEuewCD9sg3rjdsUSmkcJkUaHvjEV5CHddog2NK"
        .parse()
        .unwrap();
    assert_eq!(
        inner_actions(CREATE_ACCOUNT),
        [
            Action::create_account(),
            Action::transfer(NearToken::from_near(1)),
            Action::add_full_access_key(full_access_key),
        ]
    );
    assert_eq!(
        inner_actions(CREATE_ACCOUNT_TX_STATUS),
        [
            Action::create_account(),
            Action::transfer(NearToken::from_near(1)),
        ]
    );
}

#[test]
fn relayed_transfer_inner_action() {
    assert_eq!(
        inner_actions(TRANSFER),
        [Action::transfer(NearToken::from_yoctonear(1))]
    );
}

/// `EXPERIMENTAL_tx_status` also returns the receipts, whose delegate action
/// carries the same protocol-shaped inner actions.
#[test]
fn tx_status_receipt_delegate_inner_actions() {
    let outcome: FinalExecutionOutcome = serde_json::from_str(CREATE_ACCOUNT_TX_STATUS).unwrap();
    let delegates: Vec<_> = outcome
        .receipts
        .iter()
        .filter_map(|receipt| match &receipt.receipt {
            ReceiptContent::Action(action) => Some(&action.actions),
            _ => None,
        })
        .flatten()
        .filter_map(|action| match action {
            ActionView::Delegate {
                delegate_action, ..
            } => Some(delegate_action.clone()),
            _ => None,
        })
        .collect();
    let [delegate] = &delegates[..] else {
        panic!("expected one delegate receipt action, got {delegates:?}");
    };
    assert_eq!(
        delegate.actions,
        [
            Action::create_account(),
            Action::transfer(NearToken::from_near(1)),
        ]
    );

    // The view converts back into the signed protocol action.
    let action = DelegateAction::try_from(delegate.clone()).unwrap();
    assert_eq!(action.receiver_id.as_str(), "sub2.alice.sandbox");
    assert_eq!(action.actions.len(), 2);
}
