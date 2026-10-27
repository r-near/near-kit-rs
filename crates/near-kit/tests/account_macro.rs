//! `near_kit::account!` works for a crate that depends on near-kit only (the
//! expansion names near-kit-primitives through `$crate`).

use near_kit::{AccountId, account};

#[test]
fn account_macro_builds_a_checked_account_id() {
    let alice: AccountId = account!("alice.near");
    assert_eq!(alice, "alice.near".parse::<AccountId>().unwrap());

    let implicit = account!("248e104d1d4764d713c4211c13808c8fc887869c580f4178e60538ac5c2a0b26");
    assert_eq!(implicit.as_str().len(), 64);
}
