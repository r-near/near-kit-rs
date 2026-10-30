//! A `#[contract]` method's parameters are sent by name, as near-sdk's
//! `#[near]` methods read them and its `#[ext_contract]` methods send them,
//! unless one is marked `#[args]`.
//!
//! The expectations are near-sdk's own encoding, written out by hand: its
//! `#[ext_contract]` serializes a struct with one field per parameter,
//! borrowing each (`struct Input<'nearinput> { a: &'nearinput A, .. }`,
//! near-sdk-macros 5.30 `core_impl/code_generator/attr_sig_info.rs:50-79` and
//! `serializer.rs:5-38`), and its `#[near]` methods deserialize the same
//! struct with owned fields (`attr_sig_info.rs:97-133`).
#![cfg(feature = "contracts")]

use std::sync::{Arc, Mutex};

use borsh::BorshSerialize;
use near_kit::protocol::Action;
use near_kit::rpc::{BlockReference, ViewFunctionResult};
use near_kit::transaction::FunctionCall;
use near_kit::{
    AccountId, AccountIdRef, Contract, CryptoHash, Gas, NearToken, TxSender, Viewer, account,
};
use serde::Serialize;

#[derive(Serialize, BorshSerialize, Clone, Copy)]
pub struct DomainId(pub u64);

#[derive(Serialize, BorshSerialize, Clone)]
pub struct UpdateHash(pub [u8; 4]);

/// An args struct, sent as the whole argument object by an `#[args]`
/// parameter.
#[derive(Serialize)]
pub struct VoteContractUpdateArgs {
    pub update_hash: UpdateHash,
}

#[derive(Serialize)]
pub struct PublicKeyArgs {
    pub domain_id: Option<DomainId>,
}

#[near_kit::contract]
pub trait SignerContract {
    fn derived_public_key(
        &self,
        path: String,
        predecessor: Option<AccountId>,
        domain_id: Option<DomainId>,
    ) -> Option<String>;

    /// One parameter, sent by name like near-sdk.
    fn public_key(&self, domain_id: Option<DomainId>) -> Option<String>;

    /// The same, with an `#[args]` struct.
    fn public_key_args(&self, #[args] args: PublicKeyArgs) -> Option<String>;

    #[call(gas = Gas::from_tgas(22))]
    fn vote_contract_update(&mut self, update_hash: UpdateHash);

    /// The whole argument object.
    #[call]
    fn vote_contract_update_args(&mut self, #[args] args: VoteContractUpdateArgs);

    #[call(gas = Gas::from_tgas(30), deposit = NearToken::from_yoctonear(1))]
    fn prepay_attestation_storage(&mut self, account_id: AccountId, grants: u32);

    #[call]
    #[borsh]
    fn submit(&mut self, version: u32, code: Vec<u8>, hash: UpdateHash);

    #[call]
    #[borsh]
    fn submit_one(&mut self, code: Vec<u8>);

    #[call]
    #[borsh]
    fn submit_one_args(&mut self, #[args] code: Vec<u8>);
}

/// NEP-245, with borrowed parameters.
#[near_kit::contract]
pub trait MultiToken {
    fn mt_balance_of(&self, account_id: &AccountIdRef, token_id: &str) -> Option<String>;

    #[call(deposit = NearToken::from_yoctonear(1))]
    fn mt_transfer(
        &mut self,
        receiver_id: &AccountIdRef,
        token_id: &str,
        amount: &str,
        approval: Option<(AccountId, u64)>,
        memo: Option<&str>,
    );
}

/// Keyword and `impl Trait` parameters.
#[near_kit::contract(borsh)]
pub trait Odd {
    #[json]
    fn by_kind(&self, r#type: &str, filter: impl Serialize) -> Option<u64>;

    #[json]
    fn by_type(&self, r#type: &str) -> Option<u64>;

    #[call]
    fn record(&mut self, r#type: u8, payload: impl BorshSerialize);
}

/// Records the arguments of every view and call.
#[derive(Clone, Default)]
struct Recorder {
    args: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Recorder {
    fn last(&self) -> Vec<u8> {
        self.args.lock().unwrap().pop().expect("something was sent")
    }
}

impl Viewer for Recorder {
    type ViewError = std::convert::Infallible;

    async fn view_function(
        &self,
        _contract_id: &AccountId,
        _method: &str,
        args: &[u8],
        _block: BlockReference,
    ) -> Result<ViewFunctionResult, Self::ViewError> {
        self.args.lock().unwrap().push(args.to_vec());
        // JSON reads an empty result as `null`.
        Ok(ViewFunctionResult::new(Vec::new(), 1, CryptoHash::ZERO))
    }
}

impl TxSender for Recorder {
    type Output = ();
    type SendError = std::convert::Infallible;

    fn account_id(&self) -> &AccountId {
        unreachable!("not used by Call")
    }

    async fn send(
        &self,
        _receiver_id: AccountId,
        actions: Vec<Action>,
    ) -> Result<(), Self::SendError> {
        for action in actions {
            let Action::FunctionCall(call) = action else {
                panic!("expected a function call, got {action:?}");
            };
            self.args.lock().unwrap().push(call.args);
        }
        Ok(())
    }
}

fn contract() -> AccountId {
    account!("v1.signer")
}

fn action(call: FunctionCall) -> near_kit::protocol::FunctionCallAction {
    match call.into_action().unwrap() {
        Action::FunctionCall(call) => call,
        other => panic!("expected a function call, got {other:?}"),
    }
}

#[tokio::test]
async fn json_views_send_near_sdks_named_object() {
    #[derive(Serialize)]
    struct Input<'nearinput> {
        path: &'nearinput String,
        predecessor: &'nearinput Option<AccountId>,
        domain_id: &'nearinput Option<DomainId>,
    }

    let recorder = Recorder::default();
    let signer = SignerContract::client(recorder.clone(), contract());

    for (predecessor, domain_id) in [
        (None, None),
        (Some(account!("alice.near")), Some(DomainId(3))),
    ] {
        let path = "m/44".to_owned();
        let expected = serde_json::to_vec(&Input {
            path: &path,
            predecessor: &predecessor,
            domain_id: &domain_id,
        })
        .unwrap();
        signer
            .derived_public_key(path, predecessor, domain_id)
            .await
            .unwrap();
        assert_eq!(recorder.last(), expected);
    }

    // `None` is sent as `null`, as near-sdk sends it, never left out.
    signer
        .derived_public_key("m/44".into(), None, Some(DomainId(3)))
        .await
        .unwrap();
    assert_eq!(
        recorder.last(),
        br#"{"path":"m/44","predecessor":null,"domain_id":3}"#
    );
}

#[tokio::test]
async fn json_calls_and_constructors_send_the_same_object() {
    let recorder = Recorder::default();
    let signer = SignerContract::client(recorder.clone(), contract());

    let expected = br#"{"account_id":"node.near","grants":2}"#;
    let constructed = action(SignerContract::prepay_attestation_storage(
        account!("node.near"),
        2,
    ));
    assert_eq!(constructed.args, expected);
    // `#[call(gas, deposit)]` still applies.
    assert_eq!(constructed.gas, Gas::from_tgas(30));
    assert_eq!(constructed.deposit, NearToken::from_yoctonear(1));

    signer
        .prepay_attestation_storage(account!("node.near"), 2)
        .await
        .unwrap();
    assert_eq!(recorder.last(), expected);
}

#[tokio::test]
async fn a_single_parameter_is_sent_by_name() {
    let recorder = Recorder::default();
    let signer = SignerContract::client(recorder.clone(), contract());

    signer.public_key(Some(DomainId(1))).await.unwrap();
    assert_eq!(recorder.last(), br#"{"domain_id":1}"#);
    signer.public_key(None).await.unwrap();
    assert_eq!(recorder.last(), br#"{"domain_id":null}"#);

    let hash = UpdateHash([1, 2, 3, 4]);
    let expected = br#"{"update_hash":[1,2,3,4]}"#;
    signer.vote_contract_update(hash.clone()).await.unwrap();
    assert_eq!(recorder.last(), expected);
    let constructed = action(SignerContract::vote_contract_update(hash));
    assert_eq!(constructed.args, expected);
    assert_eq!(constructed.gas, Gas::from_tgas(22));
}

#[tokio::test]
async fn an_args_parameter_is_the_whole_argument_object() {
    let recorder = Recorder::default();
    let signer = SignerContract::client(recorder.clone(), contract());

    // The same bytes as the named parameter above, from a struct.
    signer
        .public_key_args(PublicKeyArgs {
            domain_id: Some(DomainId(1)),
        })
        .await
        .unwrap();
    assert_eq!(recorder.last(), br#"{"domain_id":1}"#);

    let args = || VoteContractUpdateArgs {
        update_hash: UpdateHash([1, 2, 3, 4]),
    };
    let expected = br#"{"update_hash":[1,2,3,4]}"#;
    signer.vote_contract_update_args(args()).await.unwrap();
    assert_eq!(recorder.last(), expected);
    assert_eq!(
        action(SignerContract::vote_contract_update_args(args())).args,
        expected
    );
}
#[tokio::test]
async fn borsh_sends_each_parameter_in_order() {
    #[derive(BorshSerialize)]
    struct Input<'nearinput> {
        version: &'nearinput u32,
        code: &'nearinput Vec<u8>,
        hash: &'nearinput UpdateHash,
    }

    let recorder = Recorder::default();
    let signer = SignerContract::client(recorder.clone(), contract());
    let code = vec![0xde, 0xad];
    let hash = UpdateHash([9; 4]);
    let expected = borsh::to_vec(&Input {
        version: &7,
        code: &code,
        hash: &hash,
    })
    .unwrap();
    assert_eq!(
        expected,
        [&[7, 0, 0, 0][..], &[2, 0, 0, 0, 0xde, 0xad], &[9; 4]].concat()
    );

    assert_eq!(
        action(SignerContract::submit(7, code.clone(), hash.clone())).args,
        expected
    );
    signer.submit(7, code.clone(), hash).await.unwrap();
    assert_eq!(recorder.last(), expected);

    // A one-field struct encodes as its field, so `#[args]` changes nothing
    // in Borsh.
    assert_eq!(
        action(SignerContract::submit_one(code.clone())).args,
        action(SignerContract::submit_one_args(code.clone())).args,
    );
    assert_eq!(
        action(SignerContract::submit_one(code.clone())).args,
        borsh::to_vec(&code).unwrap()
    );
}

#[tokio::test]
async fn borrowed_parameters_are_encoded_before_the_view_is_awaited() {
    #[derive(Serialize)]
    struct Input<'nearinput> {
        account_id: &'nearinput AccountIdRef,
        token_id: &'nearinput str,
    }

    let recorder = Recorder::default();
    let mt = MultiToken::client(recorder.clone(), contract());
    let view = {
        // Both borrows end before the view runs.
        let owner = account!("alice.near");
        let token = String::from("nep141:wrap.near");
        mt.mt_balance_of(&owner, &token)
    };
    view.await.unwrap();
    assert_eq!(
        recorder.last(),
        serde_json::to_vec(&Input {
            account_id: AccountIdRef::new_or_panic("alice.near"),
            token_id: "nep141:wrap.near",
        })
        .unwrap()
    );

    let transfer = action(MultiToken::mt_transfer(
        AccountIdRef::new_or_panic("bob.near"),
        "nep141:wrap.near",
        "10",
        None,
        Some("memo"),
    ));
    assert_eq!(
        transfer.args,
        br#"{"receiver_id":"bob.near","token_id":"nep141:wrap.near","amount":"10","approval":null,"memo":"memo"}"#
    );
    assert_eq!(transfer.deposit, NearToken::from_yoctonear(1));
}

#[tokio::test]
async fn keyword_and_impl_trait_parameters() {
    let recorder = Recorder::default();
    let odd = Odd::client(recorder.clone(), contract());

    odd.by_kind("swap", [1, 2]).await.unwrap();
    assert_eq!(recorder.last(), br#"{"type":"swap","filter":[1,2]}"#);
    odd.by_type("swap").await.unwrap();
    assert_eq!(recorder.last(), br#"{"type":"swap"}"#);

    odd.record(3, 5u16).await.unwrap();
    assert_eq!(recorder.last(), [3, 5, 0]);
}
