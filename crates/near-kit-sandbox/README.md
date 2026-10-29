# near-kit-sandbox

A local NEAR node in Docker, for tests that use
[`near-kit`](https://crates.io/crates/near-kit).

[API docs](https://docs.rs/near-kit-sandbox) · [Changelog](CHANGELOG.md)

```sh
cargo add --dev near-kit near-kit-sandbox
cargo add --dev tokio --features macros,rt-multi-thread
```

Docker must be running. The API docs cover starting a sandbox, creating
accounts, changing chain state, and choosing a nearcore version or image.

## License

Licensed under either of Apache License, Version 2.0 or MIT license, at your
option.
