use near_kit::rpc::{ViewFunction, ViewFunctionResult};
use near_kit::{CryptoHash, Error};

fn response(result: Vec<u8>) -> ViewFunctionResult {
    ViewFunctionResult {
        result,
        logs: vec!["observed".into()],
        block_height: 42,
        block_hash: CryptoHash::from_bytes([7; 32]),
    }
}

#[test]
fn json_view_is_portable_and_preserves_observation_context() {
    let view = ViewFunction::<u64>::json("balance")
        .args(serde_json::json!({"account_id": "alice.near"}))
        .unwrap();
    assert_eq!(view.method_name(), "balance");
    assert_eq!(view.args_bytes(), br#"{"account_id":"alice.near"}"#);
    let decoded = view.decode(response(b"123".to_vec())).unwrap();
    assert_eq!(decoded.result, 123);
    assert_eq!(decoded.logs, ["observed"]);
    assert_eq!(decoded.block_height, 42);
    assert_eq!(decoded.block_hash, CryptoHash::from_bytes([7; 32]));
}

#[test]
fn result_encoding_is_independent_of_argument_encoding() {
    #[derive(Debug, PartialEq, borsh::BorshSerialize, borsh::BorshDeserialize)]
    struct BorshOnly(u32);
    let view = ViewFunction::<BorshOnly>::borsh("read")
        .args(serde_json::json!({"key": 1}))
        .unwrap();
    assert_eq!(view.args_bytes(), br#"{"key":1}"#);
    assert_eq!(
        view.decode(response(borsh::to_vec(&BorshOnly(7)).unwrap()))
            .unwrap()
            .result,
        BorshOnly(7)
    );
    let view = ViewFunction::<u64>::json("read").args_borsh(7_u32).unwrap();
    assert_eq!(view.args_bytes(), 7_u32.to_le_bytes());
    assert_eq!(view.decode(response(b"7".to_vec())).unwrap().result, 7);
}

#[test]
fn empty_and_invalid_results_keep_existing_decoding_semantics() {
    let view = ViewFunction::<Option<u64>>::json("empty");
    assert_eq!(view.args_bytes(), b"{}");
    assert_eq!(view.decode(response(Vec::new())).unwrap().result, None);
    assert!(matches!(
        view.decode(response(vec![255])),
        Err(Error::Json(_))
    ));
    let view = ViewFunction::<u64>::borsh("empty");
    assert!(view.args_bytes().is_empty());
    assert!(matches!(
        view.decode(response(Vec::new())),
        Err(Error::Borsh(_))
    ));
}

#[test]
fn invalid_arguments_fail_during_construction() {
    struct Invalid;
    impl serde::Serialize for Invalid {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("invalid arguments"))
        }
    }
    impl borsh::BorshSerialize for Invalid {
        fn serialize<W: std::io::Write>(&self, _: &mut W) -> std::io::Result<()> {
            Err(std::io::Error::other("invalid arguments"))
        }
    }
    assert!(matches!(
        ViewFunction::<u64>::json("read").args(Invalid),
        Err(Error::Json(_))
    ));
    assert!(matches!(
        ViewFunction::<u64>::borsh("read").args_borsh(Invalid),
        Err(Error::Borsh(_))
    ));
}

#[cfg(feature = "contracts")]
#[test]
fn macro_generates_static_views_with_arguments_and_format_overrides() {
    #[derive(borsh::BorshSerialize, borsh::BorshDeserialize)]
    struct BorshOnly(u32);
    #[near_kit::contract]
    trait Mixed {
        fn count(&self) -> u64;
        fn balance(&self, args: serde_json::Value) -> u64;
        #[borsh]
        fn binary(&self, args: BorshOnly) -> BorshOnly;
    }
    #[near_kit::contract(borsh)]
    trait Binary {
        fn empty(&self);
        #[json]
        fn count(&self) -> u64;
    }
    assert!(Binary::empty().unwrap().args_bytes().is_empty());
    assert_eq!(Binary::count().unwrap().args_bytes(), b"{}");
    assert_eq!(Mixed::count().unwrap().args_bytes(), b"{}");
    let balance = Mixed::balance(serde_json::json!({"account_id":"alice.near"})).unwrap();
    assert_eq!(balance.method_name(), "balance");
    assert_eq!(balance.args_bytes(), br#"{"account_id":"alice.near"}"#);
    let binary = Mixed::binary(BorshOnly(7)).unwrap();
    assert_eq!(binary.args_bytes(), 7_u32.to_le_bytes());
    assert_eq!(
        binary
            .decode(response(8_u32.to_le_bytes().to_vec()))
            .unwrap()
            .result
            .0,
        8
    );
}
