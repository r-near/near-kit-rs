//! near-kit's `0s` derivation against near-global-contracts': near-kit hashes
//! in pure Rust so it can't reach a host function, and must give the same ids.
//!
//! Calling upstream here is only valid because no member of this workspace
//! enables `near-sdk/unit-testing`; with it, upstream would use the host.
#![allow(
    clippy::disallowed_methods,
    reason = "compares near-kit's derivation with upstream's"
)]

use std::collections::BTreeMap;

use near_kit_primitives::protocol::{
    DeterministicStateInitAction, GlobalContractId, StateInit, StateInitV1,
};
use near_kit_primitives::{AccountId, NearToken};

fn kit_id(state_init: &StateInit) -> AccountId {
    DeterministicStateInitAction {
        state_init: state_init.clone(),
        deposit: NearToken::ZERO,
    }
    .derive_account_id()
}

#[test]
fn matches_upstream_vectors() {
    // From near-global-contracts 0.3.2 `state_init::test`.
    let vectors = [
        (
            serde_json::json!({"V1": {
                "code": {"hash": "J86LNmZE9nHAxRqUYBZ64iCQYfeacMJhNqvb8WQmpZPE"},
                "data": {"AAEC": "AwQF"},
            }}),
            "0s48ddf87e648de3a52783ee9640e618234cadb18f",
        ),
        (
            serde_json::json!({"V1": {
                "code": {"account_id": "alice.near"},
                "data": {"AAEC": "AwQF"},
            }}),
            "0sf4d27a587616342eb45b8d785addbe6790695a2e",
        ),
    ];
    for (json, expected) in vectors {
        let state_init: StateInit = serde_json::from_value(json).unwrap();
        assert_eq!(kit_id(&state_init).as_str(), expected);
        assert_eq!(state_init.derive_account_id().as_str(), expected);
    }
}

#[test]
fn matches_upstream_on_samples() {
    let mut big = BTreeMap::new();
    for i in 0u8..40 {
        big.insert(vec![i; usize::from(i)], vec![0xff - i; 3 * usize::from(i)]);
    }
    let codes = [
        GlobalContractId::CodeHash([0; 32]),
        GlobalContractId::CodeHash([0xab; 32]),
        GlobalContractId::AccountId("publisher.near".parse().unwrap()),
        GlobalContractId::AccountId("a".repeat(64).parse().unwrap()),
    ];
    let datas = [
        BTreeMap::new(),
        BTreeMap::from([(vec![], vec![])]),
        BTreeMap::from([(b"k".to_vec(), b"v".to_vec())]),
        big,
    ];
    for code in &codes {
        for data in &datas {
            let state_init = StateInit::V1(StateInitV1 {
                code: code.clone(),
                data: data.clone(),
            });
            assert_eq!(kit_id(&state_init), state_init.derive_account_id());
        }
    }
}
