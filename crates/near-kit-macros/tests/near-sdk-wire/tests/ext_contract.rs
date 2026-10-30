//! `#[near_kit::contract]` sends the bytes near-sdk's `#[ext_contract]` sends
//! for the same signature: each parameter by name in JSON, each parameter's
//! encoding in order in Borsh, for one parameter as for several. An
//! `#[args]` struct with the same fields sends the same bytes.

use near_kit::protocol::Action;
use near_kit::transaction::FunctionCall;
use near_sdk::json_types::U128;
use near_sdk::mock::MockAction;
use near_sdk::test_utils::{VMContextBuilder, get_created_receipts};
use near_sdk::{AccountId as SdkAccountId, ext_contract, testing_env};

#[derive(serde::Serialize, borsh::BorshSerialize, Clone)]
pub struct UpdateHash(pub [u8; 4]);

mod sdk {
    use super::*;

    // Only the generated `ext_signer` module is used.
    #[allow(dead_code)]
    #[ext_contract(ext_signer)]
    pub trait Signer {
        fn derived_public_key(
            &self,
            path: String,
            predecessor: Option<SdkAccountId>,
            domain_id: Option<u64>,
        ) -> String;
        fn public_key(&self, domain_id: Option<u64>) -> String;
        fn vote_contract_update(&mut self, update_hash: UpdateHash) -> bool;
        fn prepay_attestation_storage(&mut self, account_id: SdkAccountId, grants: u32);
        fn submit(
            &mut self,
            #[serializer(borsh)] version: u32,
            #[serializer(borsh)] code: Vec<u8>,
            #[serializer(borsh)] hash: UpdateHash,
        );
        fn submit_one(&mut self, #[serializer(borsh)] code: Vec<u8>);
        fn mt_transfer(
            &mut self,
            receiver_id: SdkAccountId,
            token_id: String,
            amount: U128,
            approval: Option<(SdkAccountId, u64)>,
            memo: Option<String>,
        );
        fn by_kind(&self, r#type: String);
    }
}

/// The same signatures, near-kit side (views as `&self`, calls `#[call]`).
mod kit {
    use super::*;
    use near_kit::AccountId;

    #[derive(serde::Serialize)]
    pub struct VoteContractUpdateArgs {
        pub update_hash: UpdateHash,
    }

    #[derive(serde::Serialize)]
    pub struct PrepayArgs {
        pub account_id: AccountId,
        pub grants: u32,
    }

    #[near_kit::contract]
    pub trait Signer {
        #[call]
        fn vote_contract_update(&mut self, update_hash: UpdateHash);
        #[call]
        fn prepay_attestation_storage(&mut self, account_id: AccountId, grants: u32);
        #[call]
        #[borsh]
        fn submit(&mut self, version: u32, code: Vec<u8>, hash: UpdateHash);
        #[call]
        #[borsh]
        fn submit_one(&mut self, code: Vec<u8>);
        #[call]
        fn mt_transfer(
            &mut self,
            receiver_id: &near_kit::AccountIdRef,
            token_id: &str,
            amount: U128,
            approval: Option<(AccountId, u64)>,
            memo: Option<&str>,
        );
    }

    /// The same contract methods, with `#[args]` structs.
    #[near_kit::contract]
    pub trait SignerArgs {
        #[call]
        fn vote_contract_update(&mut self, #[args] args: VoteContractUpdateArgs);
        #[call]
        fn prepay_attestation_storage(&mut self, #[args] args: PrepayArgs);
        #[call]
        #[borsh]
        fn submit_one(&mut self, #[args] code: Vec<u8>);
    }

    // Views have no `FunctionCall` constructor; build the same call through
    // a call-only mirror to read their bytes.
    #[near_kit::contract]
    pub trait SignerViewsAsCalls {
        #[call]
        fn derived_public_key(
            &mut self,
            path: String,
            predecessor: Option<AccountId>,
            domain_id: Option<u64>,
        );
        #[call]
        fn public_key(&mut self, domain_id: Option<u64>);
        #[call]
        fn by_kind(&mut self, r#type: &str);
    }
}

/// The method name and args of the one function call `send` schedules.
fn sdk_bytes(send: impl FnOnce(SdkAccountId)) -> (String, Vec<u8>) {
    testing_env!(VMContextBuilder::new().build());
    send("v1.signer".parse().unwrap());
    let receipts = get_created_receipts();
    let [receipt] = receipts.as_slice() else {
        panic!("expected one receipt, got {}", receipts.len())
    };
    match &receipt.actions[..] {
        [
            MockAction::FunctionCallWeight {
                method_name, args, ..
            },
        ] => (
            String::from_utf8(method_name.clone()).unwrap(),
            args.clone(),
        ),
        other => panic!("expected one function call, got {other:?}"),
    }
}

fn kit_bytes(call: FunctionCall) -> (String, Vec<u8>) {
    match call.into_action().unwrap() {
        Action::FunctionCall(call) => (call.method_name, call.args),
        other => panic!("expected a function call, got {other:?}"),
    }
}

fn sdk_id(id: &str) -> SdkAccountId {
    id.parse().unwrap()
}

fn kit_id(id: &str) -> near_kit::AccountId {
    id.parse().unwrap()
}

#[test]
fn several_parameters() {
    for (predecessor, domain_id) in [(None, None), (Some("alice.near"), Some(3))] {
        let sdk = sdk_bytes(|id| {
            let _ = sdk::ext_signer::ext(id).derived_public_key(
                "m/44'/60'".into(),
                predecessor.map(sdk_id),
                domain_id,
            );
        });
        let kit = kit_bytes(kit::SignerViewsAsCalls::derived_public_key(
            "m/44'/60'".into(),
            predecessor.map(kit_id),
            domain_id,
        ));
        assert_eq!(sdk, kit);
    }

    let sdk = sdk_bytes(|id| {
        let _ = sdk::ext_signer::ext(id).prepay_attestation_storage(sdk_id("node.near"), 2);
    });
    assert_eq!(
        sdk,
        kit_bytes(kit::Signer::prepay_attestation_storage(
            kit_id("node.near"),
            2
        ))
    );
    assert_eq!(
        sdk,
        kit_bytes(kit::SignerArgs::prepay_attestation_storage(
            kit::PrepayArgs {
                account_id: kit_id("node.near"),
                grants: 2,
            }
        ))
    );

    for (approval, memo) in [(None, None), (Some(("carol.near", 7)), Some("hi"))] {
        let sdk = sdk_bytes(|id| {
            let _ = sdk::ext_signer::ext(id).mt_transfer(
                sdk_id("bob.near"),
                "nep141:wrap.near".into(),
                U128(10u128.pow(24)),
                approval.map(|(a, n)| (sdk_id(a), n)),
                memo.map(Into::into),
            );
        });
        let kit = kit_bytes(kit::Signer::mt_transfer(
            near_kit::AccountIdRef::new_or_panic("bob.near"),
            "nep141:wrap.near",
            U128(10u128.pow(24)),
            approval.map(|(a, n)| (kit_id(a), n)),
            memo,
        ));
        assert_eq!(sdk, kit);
    }
}

#[test]
fn one_parameter() {
    for domain_id in [None, Some(1)] {
        let sdk = sdk_bytes(|id| {
            let _ = sdk::ext_signer::ext(id).public_key(domain_id);
        });
        assert_eq!(
            sdk,
            kit_bytes(kit::SignerViewsAsCalls::public_key(domain_id))
        );
    }

    let sdk = sdk_bytes(|id| {
        let _ = sdk::ext_signer::ext(id).vote_contract_update(UpdateHash([1, 2, 3, 4]));
    });
    assert_eq!(
        sdk,
        kit_bytes(kit::Signer::vote_contract_update(UpdateHash([1, 2, 3, 4])))
    );
    assert_eq!(
        sdk,
        kit_bytes(kit::SignerArgs::vote_contract_update(
            kit::VoteContractUpdateArgs {
                update_hash: UpdateHash([1, 2, 3, 4]),
            }
        ))
    );

    // A raw identifier's key has no `r#`.
    let sdk = sdk_bytes(|id| {
        let _ = sdk::ext_signer::ext(id).by_kind("swap".into());
    });
    assert_eq!(sdk, kit_bytes(kit::SignerViewsAsCalls::by_kind("swap")));
}

#[test]
fn borsh_parameters() {
    let sdk = sdk_bytes(|id| {
        let _ = sdk::ext_signer::ext(id).submit(7, vec![0xde, 0xad], UpdateHash([9; 4]));
    });
    assert_eq!(
        sdk,
        kit_bytes(kit::Signer::submit(7, vec![0xde, 0xad], UpdateHash([9; 4])))
    );

    let sdk = sdk_bytes(|id| {
        let _ = sdk::ext_signer::ext(id).submit_one(vec![1, 2, 3]);
    });
    assert_eq!(sdk, kit_bytes(kit::Signer::submit_one(vec![1, 2, 3])));
    assert_eq!(sdk, kit_bytes(kit::SignerArgs::submit_one(vec![1, 2, 3])));
}
