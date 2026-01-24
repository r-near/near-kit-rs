# near-kit Examples

Runnable examples demonstrating near-kit features.

## Examples

### [`quickstart.rs`](./quickstart.rs)

Essential operations: balance, view, call, transfer, multi-action transactions, typed contracts.

**Start here if you're new to near-kit.**

```bash
# View operations (no credentials needed)
cargo run --example quickstart

# All operations (requires testnet credentials)
NEAR_ACCOUNT_ID=your-account.testnet \
NEAR_PRIVATE_KEY=ed25519:... \
cargo run --example quickstart
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

### [`rotating_signer.rs`](./rotating_signer.rs)

High-throughput concurrent transactions using multiple access keys to avoid nonce collisions.

Requires the `sandbox` feature (starts a local NEAR node):

```bash
cargo run --example rotating_signer --features sandbox
```

## Getting Testnet Credentials

1. Create an account at [testnet.mynearwallet.com](https://testnet.mynearwallet.com/)
2. Export your credentials:
   - The private key is stored in `~/.near-credentials/testnet/your-account.testnet.json`
   - Or use `near-cli`: `near account export-account your-account.testnet`

## Documentation

Full API docs: [docs.rs/near-kit](https://docs.rs/near-kit)
