# near-kit Examples

Runnable examples demonstrating near-kit features.

## Examples

### [`quickstart.rs`](./quickstart.rs)

Essential operations: balance, view, call, transfer, multi-action transactions, typed contracts.

**Start here if you're new to near-kit.**

```bash
# View operations (no credentials needed)
cargo run --example quickstart --features contracts

# All operations (requires testnet credentials)
NEAR_ACCOUNT_ID=your-account.testnet \
NEAR_PRIVATE_KEY=ed25519:... \
cargo run --example quickstart --features contracts
```

### [`meta_transactions.rs`](./meta_transactions.rs)

Gasless transactions (NEP-366): user signs off-chain, relayer pays gas.

```bash
USER_ACCOUNT_ID=user.testnet \
USER_PRIVATE_KEY=ed25519:... \
RELAYER_ACCOUNT_ID=relayer.testnet \
RELAYER_PRIVATE_KEY=ed25519:... \
cargo run --example meta_transactions
```

### [`rotating_signer.rs`](../../near-kit-sandbox/examples/rotating_signer.rs)

High-throughput concurrent transactions using multiple access keys to avoid nonce collisions.

Lives in the `near-kit-sandbox` package and starts a local NEAR node through Docker:

```bash
cargo run -p near-kit-sandbox --example rotating_signer --features integration-tests
```

### [`sequential_sends.rs`](../../near-kit-sandbox/examples/sequential_sends.rs)

Per-key sequential transaction execution using `into_per_key_signers()`, preventing nonce ordering issues.

```bash
cargo run -p near-kit-sandbox --example sequential_sends --features integration-tests
```

### [`global_contracts.rs`](../../near-kit-sandbox/examples/global_contracts.rs)

Publish contracts to the global registry and deploy them to other accounts using `deploy_from`. Demonstrates both `PublishMode::Updatable` (by publisher) and `PublishMode::Immutable` (by hash).

```bash
cargo run -p near-kit-sandbox --example global_contracts --features integration-tests
```

## Getting Testnet Credentials

1. Create an account at [testnet.mynearwallet.com](https://testnet.mynearwallet.com/)
2. Export your credentials:
   - The private key is stored in `~/.near-credentials/testnet/your-account.testnet.json`
   - Or use `near-cli`: `near account export-account your-account.testnet`

## Documentation

Full API docs: [docs.rs/near-kit](https://docs.rs/near-kit)
