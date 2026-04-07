//! WASM integration tests — run with:
//!
//! ```sh
//! wasm-pack test --headless --chrome --no-default-features -p near-kit
//! ```

#![cfg(target_arch = "wasm32")]

use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

use near_kit::{Near, NearToken};

#[wasm_bindgen_test]
async fn test_view_balance() {
    let near = Near::testnet().build();
    let balance = near.balance("near.testnet").await.unwrap();
    // near.testnet is a system account, should always have some balance
    assert!(balance.available > NearToken::from_near(0));
}

#[wasm_bindgen_test]
async fn test_view_call() {
    let near = Near::testnet().build();
    let exists = near.account_exists("near.testnet").await.unwrap();
    assert!(exists);
}

#[wasm_bindgen_test]
async fn test_account_not_found() {
    let near = Near::testnet().build();
    let exists = near
        .account_exists("this-account-definitely-does-not-exist-12345.testnet")
        .await
        .unwrap();
    assert!(!exists);
}

#[wasm_bindgen_test]
fn test_key_generation() {
    // Crypto operations (key gen uses OsRng → getrandom/js on wasm)
    let key = near_kit::SecretKey::generate_ed25519();
    let public = key.public_key();
    assert!(public.to_string().starts_with("ed25519:"));
}

#[wasm_bindgen_test]
fn test_nonce_generation() {
    // Tests both js_sys::Date::now() and OsRng on wasm
    let nonce = near_kit::nep413::generate_nonce();
    assert_eq!(nonce.len(), 32);
    // Timestamp portion should be non-zero
    let ts = near_kit::nep413::extract_timestamp_from_nonce(&nonce);
    assert!(ts > 0);
}
