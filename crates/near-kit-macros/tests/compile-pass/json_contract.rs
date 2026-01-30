//! Test that JSON contract macro generates valid code.

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
    fn get_messages(&self) -> Vec<Message>;
    fn total_messages(&self) -> u32;

    #[call]
    fn add_message(&mut self, args: AddMessageArgs);
}

fn main() {
    // Verify the generated client can be constructed
    let near = Near::testnet().build();
    let client = JsonGuestbookClient::new(&near, "guestbook.testnet".parse().unwrap());

    // Verify methods exist and have correct return types
    let _view: ViewCall<Vec<Message>> = client.get_messages();
    let _view: ViewCall<u32> = client.total_messages();
    let _call: CallBuilder = client.add_message(AddMessageArgs {
        text: "hello".to_string(),
    });
}
