# near-kit-macros

The `#[contract]` proc macro behind [`near-kit`](https://crates.io/crates/near-kit)'s
typed contract clients. It turns a Rust trait describing a contract's methods
into a client whose view methods run on any `Viewer` and whose `#[call]`
methods send through any `TxSender`, plus offline `FunctionCall` constructors.

Don't depend on this crate directly. Use it through one of:

- `near-kit` with the `contracts` feature: `#[near_kit::contract]`
  (`cargo add near-kit --features contracts`)
- `near-kit-primitives` with the `contracts` feature, for crates without the
  RPC client, such as contracts built for wasm

The [typed contracts and tokens guide](https://docs.rs/near-kit/latest/near_kit/guide/contracts_and_tokens/index.html)
covers the trait syntax, argument formats and generated API.

## License

Licensed under either Apache-2.0 or MIT, at your option.
