# near-kit-primitives

NEAR Protocol types, keys, signing and transaction building, with no RPC
client, no async runtime and no HTTP stack.

[API docs](https://docs.rs/near-kit-primitives/0.30.0-rc.1) · [near-kit](https://crates.io/crates/near-kit)

This is the offline core of [`near-kit`](https://crates.io/crates/near-kit),
which re-exports every item at the same path under `near_kit::`. Use it
directly where a network stack can't go: a smart contract, a signing
service, a tool that decodes RPC responses.

```sh
cargo add near-kit-primitives@0.30.0-rc.1
cargo add near-kit-primitives@0.30.0-rc.1 --features contracts   # the #[contract] macro
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
