//! Test that contract composition via trait inheritance works.
//!
//! This tests the ergonomics of:
//! - Standard traits generating Methods + TxMethods traits
//! - Composite traits inheriting methods from supertraits
//! - `.typed_call::<dyn T>()` on TransactionBuilder
//! - Blanket impls providing methods automatically

use near_kit::*;
use serde::Serialize;

// ── Define two "standards" ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct StorageDepositArgs {
    pub account_id: Option<String>,
}

#[near_kit::contract]
pub trait StorageManagement {
    #[call]
    fn storage_deposit(&mut self, args: StorageDepositArgs);
}

#[derive(Serialize)]
pub struct FtTransferArgs {
    pub receiver_id: String,
    pub amount: String,
}

#[near_kit::contract]
pub trait FungibleToken {
    fn ft_balance_of(&self) -> String;

    #[call]
    fn ft_transfer(&mut self, args: FtTransferArgs);
}

// ── Define a composite contract ──────────────────────────────────────────

#[derive(Serialize)]
pub struct MintArgs {
    pub amount: String,
}

#[near_kit::contract]
pub trait MyToken: FungibleToken + StorageManagement {
    #[call]
    fn mint(&mut self, args: MintArgs);
}

fn main() {
    let near = Near::testnet().build();

    // ── 1. Per-trait client still works ───────────────────────────────
    let ft_client = FungibleTokenClient::new(near.clone(), "token.near".parse().unwrap());
    let _view: ViewCall<String> = ft_client.ft_balance_of();
    let _call: CallBuilder = ft_client.ft_transfer(FtTransferArgs {
        receiver_id: "bob.near".to_string(),
        amount: "100".to_string(),
    });

    // ── 2. Composite client has its OWN methods ──────────────────────
    let token = MyTokenClient::new(near.clone(), "token.near".parse().unwrap());
    let _call: CallBuilder = token.mint(MintArgs {
        amount: "1000".to_string(),
    });

    // ── 3. Composite client gets supertrait methods via blanket impls ─
    // FungibleToken methods (via ImplementsFungibleToken marker)
    let _view: ViewCall<String> = token.ft_balance_of();
    let _call: CallBuilder = token.ft_transfer(FtTransferArgs {
        receiver_id: "bob.near".to_string(),
        amount: "100".to_string(),
    });

    // StorageManagement methods (via ImplementsStorageManagement marker)
    let _call: CallBuilder = token.storage_deposit(StorageDepositArgs {
        account_id: Some("bob.near".to_string()),
    });

    // ── 4. .typed_call::<dyn T>() on TransactionBuilder ──────────────
    // Per-standard TxBuilder
    let _: FungibleTokenTxBuilder = near
        .transaction("token.near")
        .typed_call::<dyn FungibleToken>();

    // Composite TxBuilder
    let _: MyTokenTxBuilder = near
        .transaction("token.near")
        .typed_call::<dyn MyToken>();

    // ── 5. Typed call methods on TxBuilder ───────────────────────────
    // Direct standard methods
    let _: CallBuilder = near
        .transaction("token.near")
        .typed_call::<dyn FungibleToken>()
        .ft_transfer(FtTransferArgs {
            receiver_id: "bob.near".to_string(),
            amount: "100".to_string(),
        });

    // Composite TxBuilder gets own methods
    let _: CallBuilder = near
        .transaction("token.near")
        .typed_call::<dyn MyToken>()
        .mint(MintArgs {
            amount: "1000".to_string(),
        });

    // Composite TxBuilder gets supertrait methods via blanket impls
    let _: CallBuilder = near
        .transaction("token.near")
        .typed_call::<dyn MyToken>()
        .ft_transfer(FtTransferArgs {
            receiver_id: "bob.near".to_string(),
            amount: "100".to_string(),
        });

    let _: CallBuilder = near
        .transaction("token.near")
        .typed_call::<dyn MyToken>()
        .storage_deposit(StorageDepositArgs {
            account_id: Some("bob.near".to_string()),
        });

    // ── 6. Chaining typed calls across standards ─────────────────────
    // This is the key ergonomic win: fluent cross-standard composition
    let _: CallBuilder = near
        .transaction("token.near")
        .typed_call::<dyn MyToken>()
        .storage_deposit(StorageDepositArgs {
            account_id: Some("bob.near".to_string()),
        })
        .typed_call::<dyn MyToken>()
        .ft_transfer(FtTransferArgs {
            receiver_id: "bob.near".to_string(),
            amount: "100".to_string(),
        });
}
