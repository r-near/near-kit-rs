//! Public keys decode as leniently as nearcore: only the type tag and length
//! are checked, never the curve point. Keys that the chain hands back must
//! always decode and re-encode byte-for-byte.

use near_kit::signer::{ParseKeyError, PublicKey, PublicKeyHandle};

/// 32 bytes of the right length that are not a valid compressed ed25519 point
/// (this y-coordinate has no corresponding x).
fn off_curve_ed25519() -> PublicKey {
    let mut bytes = [0u8; 32];
    bytes[0] = 2;
    PublicKey::Ed25519(bytes)
}

#[test]
fn off_curve_ed25519_key_round_trips_through_string_borsh_and_json() {
    let key = off_curve_ed25519();
    assert_eq!(key.validate(), Err(ParseKeyError::InvalidCurvePoint));

    // String
    let s = key.to_string();
    let parsed: PublicKey = s.parse().unwrap();
    assert_eq!(parsed, key);
    assert_eq!(parsed.to_string(), s);

    // Borsh
    let bytes = borsh::to_vec(&key).unwrap();
    assert_eq!(bytes.len(), 33);
    assert_eq!(bytes[0], 0);
    let decoded: PublicKey = borsh::from_slice(&bytes).unwrap();
    assert_eq!(decoded, key);
    assert_eq!(borsh::to_vec(&decoded).unwrap(), bytes);

    // JSON
    let json = serde_json::to_string(&key).unwrap();
    assert_eq!(json, format!("\"{s}\""));
    let from_json: PublicKey = serde_json::from_str(&json).unwrap();
    assert_eq!(from_json, key);

    // Wrappers decode it too.
    let handle: PublicKeyHandle = s.parse().unwrap();
    assert_eq!(handle.full_pubkey(), Some(&key));
    let handle: PublicKeyHandle = serde_json::from_str(&json).unwrap();
    assert_eq!(handle.into_full(), Some(key));
}

#[test]
fn checked_constructor_still_rejects_off_curve_ed25519() {
    let PublicKey::Ed25519(bytes) = off_curve_ed25519() else {
        unreachable!()
    };
    assert_eq!(
        PublicKey::ed25519_from_bytes(bytes),
        Err(ParseKeyError::InvalidCurvePoint)
    );
}

#[test]
fn decoding_still_checks_tag_and_length() {
    assert!(matches!(
        "ed25519:1111".parse::<PublicKey>(),
        Err(ParseKeyError::InvalidLength { .. })
    ));
    assert!(matches!(
        format!("p256:{}", bs58::encode([2u8; 32]).into_string()).parse::<PublicKey>(),
        Err(ParseKeyError::UnknownKeyType(_))
    ));
    assert!(borsh::from_slice::<PublicKey>(&[0u8; 32]).is_err());
}

#[cfg(feature = "rpc")]
mod rpc {
    use near_kit::Near;
    use near_kit::rpc::{BoxFuture, RpcError, RpcTransport, TransportResponse};
    use serde_json::json;

    use super::*;

    struct ResponseTransport(Vec<u8>);

    impl RpcTransport for ResponseTransport {
        fn post_json(
            &self,
            _url: &str,
            _body: Vec<u8>,
        ) -> BoxFuture<'_, Result<TransportResponse, RpcError>> {
            let body = self.0.clone();
            Box::pin(async move { Ok(TransportResponse { status: 200, body }) })
        }
    }

    #[tokio::test]
    async fn access_key_list_with_off_curve_key_decodes() {
        let key = off_curve_ed25519();
        let response = serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "keys": [
                    {
                        "public_key": key.to_string(),
                        "access_key": { "nonce": 7, "permission": "FullAccess" }
                    },
                    {
                        "public_key": "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp",
                        "access_key": { "nonce": 8, "permission": "FullAccess" }
                    }
                ],
                "block_height": 42,
                "block_hash": "11111111111111111111111111111111"
            }
        }))
        .unwrap();
        let near = Near::testnet()
            .transport(ResponseTransport(response))
            .build();

        let list = near.access_keys("alice.testnet").await.unwrap();
        assert_eq!(list.keys.len(), 2);
        assert_eq!(list.keys[0].public_key.full_pubkey(), Some(&key));
        assert!(list.keys[0].public_key.refers_to(&key));
        assert!(list.keys[1].public_key.full_pubkey().is_some());
    }
}
