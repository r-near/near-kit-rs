# near-kit-macros

The `#[contract]` proc macro behind [`near-kit`](https://crates.io/crates/near-kit)'s
typed contract clients. It turns a Rust trait describing a contract's methods
into a client whose view methods run on any `Viewer` and whose `#[call]`
methods send through any `TxSender`, plus offline `FunctionCall` constructors.

Each method's parameters are the contract method's, sent by name as
near-sdk's `#[near]` and `#[ext_contract]` encode them, so a trait can copy a
near-sdk contract's signatures:

```rust
#[near_kit::contract]
pub trait Guestbook {
    // get_messages(None, Some("10")) sends {"from_index":null,"limit":"10"}
    fn get_messages(&self, from_index: Option<&str>, limit: Option<&str>) -> Vec<Message>;

    // add_message("hello") sends {"text":"hello"}
    #[call]
    fn add_message(&mut self, text: &str);
}
```

To send a struct as the whole argument object instead, mark its parameter
`#[args]`: `fn add_message(&mut self, #[args] args: AddMessage)`.

Don't depend on this crate directly. Use it through one of:

- `near-kit` with the `contracts` feature: `#[near_kit::contract]`
  (`cargo add near-kit@0.30.0-rc.3 --features contracts`)
- `near-kit-primitives` with the `contracts` feature, for crates without the
  RPC client, such as contracts built for wasm

The [typed contracts and tokens guide](https://docs.rs/near-kit/0.30.0-rc.3/near_kit/guide/contracts_and_tokens/index.html)
covers the trait syntax, argument formats and generated API.

## License

Licensed under either Apache-2.0 or MIT, at your option.
