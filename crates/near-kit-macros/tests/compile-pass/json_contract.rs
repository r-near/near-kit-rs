//! Test that JSON contract macro generates valid code.

use near_kit::transaction::{Call, FunctionCall, View};
use near_kit::signer::{InMemorySigner, SecretKey};
use near_kit::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct AddMessageArgs {
    pub text: String,
}

/// JSON contract (default format)
#[near_kit::contract]
pub trait JsonGuestbook {
    /// Every message, oldest first.
    fn get_messages(&self) -> Vec<Message>;
    fn total_messages(&self) -> u32;

    /// Append a message.
    #[call]
    fn add_message(&mut self, args: AddMessageArgs);
}

fn main() {
    let near = Near::testnet().build();
    // `JsonGuestbookClient` defaults its backend to `Near`.
    let client: JsonGuestbookClient = near.contract::<JsonGuestbook>(account!("guestbook.testnet"));
    let _: &AccountId = client.contract_id();

    let _view: View<Near, Vec<Message>> = client.get_messages();
    let _view: View<Near, u32> = client.total_messages().at(near_kit::rpc::Finality::Optimistic);
    // Calls need a sender: the same client over `SigningNear`.
    let account: SigningNear = near.with_signer(
        InMemorySigner::from_secret_key(account!("alice.testnet"), SecretKey::generate_ed25519())
            .unwrap(),
    );
    let signing_client: JsonGuestbookClient<SigningNear> =
        account.contract::<JsonGuestbook>(account!("guestbook.testnet"));
    let _call: Call<SigningNear> = signing_client
        .add_message(AddMessageArgs {
            text: "hello".to_string(),
        })
        .gas(Gas::from_tgas(50))
        .deposit(NearToken::from_yoctonear(1));

    // The client can also be built from the trait, for any backend.
    let _: JsonGuestbookClient<Near> =
        <JsonGuestbook as Contract>::client(near.clone(), account!("guestbook.testnet"));

    // Static FunctionCall constructors on the unit struct
    let _fc: FunctionCall = JsonGuestbook::add_message(AddMessageArgs {
        text: "composable".to_string(),
    });

    // FunctionCall composes into a transaction via add_action
    let _tx = account.transaction(account!("guestbook.testnet")).add_action(
        JsonGuestbook::add_message(AddMessageArgs {
            text: "composed".to_string(),
        })
        .gas(Gas::from_tgas(50)),
    );
}
