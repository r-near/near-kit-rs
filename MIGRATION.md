# Upgrading from near-kit 0.20.1 to 0.30

## What changed and why

This release is a clean break with no deprecated aliases. `Near` only reads;
`near.with_signer(signer)` returns a `SigningNear` that sends a receiver plus
`Action` values. Arguments are typed (`AccountId`, `NearToken`, `Gas`), and
`account!("..")` checks a literal at compile time. The protocol types moved
into `near-kit-primitives`, re-exported at the same `near_kit::` paths. Each
layer has its own error type that converts into `near_kit::Error` with `?`.
The [guide](https://docs.rs/near-kit/latest/near_kit/guide/index.html)
explains the model.

## Where did this go?

The compiler reports a removed method as ``no method named `x` found`` with no
hint where it went. Look it up here; the sections below have the details. In
the right-hand column `near` is a `Near`, `alice` a `SigningNear`, and
`block` anything that converts into a `BlockReference` (a `Finality`, a block
height, a block hash or a `SyncCheckpoint`).

Some behaviour changed without a compile error: sends wait for `Final` by
default, cached nonces are shared per chain, an Ed25519 secret key prints in
its 64-byte form, and some RPC errors are no longer retried. The sections
after the table cover each one.

### Clients

| 0.20.1 | This release |
|---|---|
| `Near` (reads and, with a signer, sends) | `Near` reads; `SigningNear` reads and sends |
| `Near::with_signer(s) -> Near` | `near.with_signer(s) -> SigningNear` |
| `NearBuilder::credentials(key, id)`, `NearBuilder::signer(s)` | `near.with_signer(InMemorySigner::new(id, key.parse()?))` after `build()` |
| `Near::from_env()` | `SigningNear::from_env()`; with a URL in `NEAR_NETWORK` it also needs `NEAR_CHAIN_ID` |
| `Near::from_rpc(rpc, chain_id)` | `Near::custom(url, chain_id).transport(t).retry_config(c).build()` |
| `NearBuilder::chain_id(id)` | `Near::custom(url, id)`; a preset's chain ID is fixed, and `near.chain_id()` reads it |
| `NearBuilder::http_client(client)` | `.transport(ReqwestTransport::with_client(client))`, with `client` built from `near_kit::reqwest` |
| `Near::sandbox(&sandbox)`, `rpc::SandboxNetwork` | `sandbox.root_account()` |
| `Near::account_id()`, `Near::try_account_id()` | `alice.account_id()`; a `Near` has no account |
| `Near::signer()`, `Near::public_key()` | `alice.signer()`, `alice.signer().public_key()` |
| `Near::rpc_url()` | `near.rpc().url()` |
| `Near::from(builder)`, `builder.into()` | `builder.build()` |
| `Near::max_nonce_retries(n)` | `NearBuilder::max_nonce_retries(n)` before `build()`, or `alice.clone().max_nonce_retries(n)` |
| `Near::sign_message(params)` | `alice.signer().key().sign_nep413(alice.account_id(), &params).await?` |
| `Near::send(&signed)` | `near.send_signed(&signed)` |
| `Near::balance`, `account_exists`, `view`, `tx_status`, `contract`, `chain_id`, `rpc` called on a signing client | Still on `Near`; from a `SigningNear`, `alice.near().balance(id)` and so on. `alice.contract::<T>(id)` stays. |
| `Near::with_signer(s)` on a client that already had a signer | `alice.near().with_signer(s)` |

### Sending transactions

| 0.20.1 | This release |
|---|---|
| `Near::transaction(rx)`, `TransactionBuilder` | `alice.transaction(rx)`, a `Tx` |
| `Near::transfer(id, amount)` | `alice.transaction(id).add_action(Action::transfer(amount))` |
| `Near::call(id, m)` | `alice.transaction(id).add_action(FunctionCall::new(m))`, or a typed contract client |
| `Near::deploy(wasm)`, `Near::publish(code, mode)`, `Near::deploy_from(id)`, `Near::add_full_access_key(pk)`, `Near::delete_key(pk)` | `alice.transaction(alice.account_id()).add_action(Action::deploy_contract(wasm))`, and `Action::publish`, `Action::deploy_from`, `Action::add_full_access_key`, `Action::delete_key` |
| `Near::state_init(si, deposit)` | `let init = DeterministicStateInitAction { state_init: si, deposit };` then `alice.transaction(init.derive_account_id()).add_action(Action::DeterministicStateInit(init))` (see Platforms and features for why not `si.derive_account_id()`) |
| `Near::universal_state_init(usi, deposit)` | `alice.transaction(usi.derive_account_id()).add_action(Action::universal_state_init(usi, deposit))` for a typed `UniversalStateInit`; for a `RawStateInit`, `raw.try_derive_account_id()` returns a `StateInitError`, which doesn't convert into `Error` |
| `TransactionBuilder::create_account()`, `transfer`, `deploy`, `add_full_access_key`, `add_function_call_key`, `delete_key`, `delete_account`, `stake`, `publish`, `deploy_from`, `state_init`, `universal_state_init` | `Tx::add_action(Action::create_account())` and so on: one `Action::*` constructor per helper (`deploy` is `Action::deploy_contract`) |
| `TransactionBuilder::call(m)`, `CallBuilder` (`args`, `args_borsh`, `args_raw`, `gas`, `deposit`) | `add_action(FunctionCall::new(m).args(a).gas(g).deposit(d))` |
| `CallBuilder::finish()` | Not needed: `add_action` returns the `Tx` |
| `CallBuilder::into_call()` | Build the `FunctionCall` directly; from a typed `Call`, `FunctionCall::from(call)` |
| `CallBuilder::into_action()` | `function_call.into_action()?`, or `FunctionCall::from(call).into_action()?` |
| `TransactionBuilder::add_actions(iter)` | `add_action` in a loop, or `TxSender::send(&alice, rx, actions)` |
| `TransactionBuilder::send()`, `CallBuilder::send()`, `TransactionSend` | `.await` the `Tx` |
| `TransactionBuilder::wait_until::<W>()`, `TransactionSend::wait_until::<W>()` | `alice.wait_until::<W>().transaction(rx)` |
| `TransactionBuilder::max_nonce_retries(n)`, `TransactionSend::max_nonce_retries(n)` | `alice.clone().max_nonce_retries(n).transaction(rx)` |
| `TransactionBuilder::sign_with(other)` | `alice.near().with_signer(other).transaction(rx)` |
| `TransactionBuilder::sign()` | `alice.sign(rx, actions).await?` |
| `TransactionBuilder::build()`, `build_offline(..)`, `sign_offline(..)`, `into_parts()` | Keep the receiver and `Vec<Action>` yourself; `Transaction::new(signer_id, public_key, nonce, receiver_id, block_hash, actions).sign(&secret_key)` |
| `Action::deploy_from_hash(hash)`, `Action::deploy_from_account(id)` | `Action::deploy_from(hash)`, `Action::deploy_from(id)` |
| `Transaction::complete(sig)`, `TransactionV1::complete(sig)` | `into_signed(sig)` |

### Meta-transactions

| 0.20.1 | This release |
|---|---|
| `TransactionBuilder::delegate(opts)` | `alice.sign_delegate(rx, actions, opts).await?` |
| `DelegateResult` (`signed_delegate_action`, `payload`, `sender_id()`, `receiver_id()`, `to_bytes()`) | `sign_delegate` returns the `SignedDelegateAction`, which has `sender_id()`, `receiver_id()`, `to_bytes()` and `to_base64()` (the old `payload`) |
| `DelegateOptions::with_offset(n)`, `DelegateOptions::with_max_height(h)` | `DelegateOptions { block_height_offset: Some(n), ..Default::default() }`, `max_block_height: Some(h)` |
| `TransactionBuilder::signed_delegate_action(signed)` | `relayer.transaction(signed.sender_id()).add_action(Action::delegate(signed))` |
| `DelegateAction::sign(sig)`, `VersionedDelegateActionPayload::sign(sig)` | `into_signed(sig)` |
| `protocol::DELEGATE_ACTION_PREFIX`, `protocol::DELEGATE_V2_ACTION_PREFIX` | Crate-private (2^30 + 366 and 2^30 + 611); `DelegateAction::serialize_for_signing()` and `VersionedDelegateActionPayload::serialize_for_signing()` prepend them |
| `protocol::DelegateDecodeError` | `protocol::DecodeDelegateError`, now `#[non_exhaustive]` (a `match` needs a `_` arm); `Base64` holds the message as a `String` |
| `DelegateActionView::actions`, `DelegateActionV2View::actions` as `Vec<ActionView>` | `Vec<Action>` |

### Reads and blocks

| 0.20.1 | This release |
|---|---|
| `.at_block(h)`, `.at_block_hash(hash)`, `.finality(f)`, `.block_reference(r)` on `ViewCall`, `ViewCallBorsh`, `BalanceQuery`, `AccountExistsQuery` and the query builders below | `.at(h)`, `.at(hash)`, `.at(f)`, `.at(r)` |
| `BlockReference::at_height(h)`, `at_hash(hash)`, `final_()`, `near_final()`, `optimistic()` | Pass `h`, `hash` or `Finality::Final` to `.at(..)` or an `RpcClient` read; `BlockReference::from(h)` where a value is needed |
| `BlockReference::genesis()`, `BlockReference::earliest_available()` | `SyncCheckpoint::Genesis`, `SyncCheckpoint::EarliestAvailable` |
| `BlockReference::to_rpc_params()` | Removed; `RpcClient` builds the params |
| `ViewCall<T>`, `ViewCallBorsh<T>` | `View<C, T>` (crate root) |
| `ViewCall::borsh()` | `View::<_, T>::borsh(near.clone(), id, m)` |
| `Near::account(id)`, `AccountQuery` | `near.rpc().view_account(&id, block)` |
| `Near::access_keys(id)`, `AccessKeysQuery`, `AccessKeysQuery::page(..)` | `near.rpc().view_access_key_list(&id, block)`, `view_access_key_list_page(..)` |
| `Near::contract_code(id)`, `ContractCodeQuery`, `ContractCodeQuery::exists()` | `near.rpc().view_code(&id, block)`; for `exists`, treat `RpcError::ContractNotDeployed` and `RpcError::AccountNotFound` as `false` |
| `Near::global_contract(id)`, `GlobalContractQuery`, `GlobalContractQuery::exists()` | `near.rpc().view_global_contract_code(id, block)`; for `exists`, treat `RpcError::GlobalContractNotFound` as `false` |
| `Near::validators()` | `near.rpc().validators(None)` |
| `ViewFunctionResult::as_string()` | `ViewFunctionResult::text()` |
| `GasPrice::as_u128()` | `gas_price.gas_price.as_yoctonear()` |
| `rpc::LightClientBlockView`, `LightClientBlockLiteView`, `BlockHeaderInnerLiteView`, `StateChangeWithCauseView`, `StateChangeCauseView`, `StateChangeValueView` | Removed: no method returned them. Use `near.rpc().request::<_, serde_json::Value>(method, params)` or your own serde type |
| `rpc::ValidatorStakeView` (an enum with one `V1` variant: `into_v1()`, `account_id()`, `stake()`), `rpc::ValidatorStakeViewV1` | `rpc::ValidatorStakeView`, now the struct: read `.account_id`, `.public_key` and `.stake` |
| `ChunkHeaderView::proposed_split: Option<Option<TrieSplit>>` | `Option<TrieSplit>`: the inner `None` never occurred (a JSON `null` decoded as the outer `None`) |
| `rpc::TxExecutionStatus::is_executed()`, `is_block_final()`, `is_final()` | `matches!(s, ExecutedOptimistic \| Executed \| Final)`, `matches!(s, IncludedFinal \| Executed \| Final)`, `s == TxExecutionStatus::Final` |
| `PartialOrd for rpc::TxExecutionStatus` (`<`, `partial_cmp`) | `matches!` on the levels you accept, as in the row above |
| `ViewFunctionResult::bytes()` | The `result` field: `&view.result` |
| `ViewFunctionResult { result, logs, block_height, block_hash }` (a struct literal) | `ViewFunctionResult::new(result, block_height, block_hash).with_logs(logs)`: the struct is `#[non_exhaustive]`; its fields stay readable |
| `ViewStateAllResult::into_values()` | The `values` field: `all.values` |
| `Deref<Target = [StateItem]>` for `ViewStateAllResult` (`all.len()`, `all.iter()`, `all[0]`) | The `values` field: `all.values.len()`, `all.values[0]`; `for item in &all` still works |
| `AccountState::is_initialized()` | `view.state == AccountState::Initialized` |
| `ExecutionStatus::success_value()` | `match status { ExecutionStatus::SuccessValue(v) => Some(v), _ => None }` |
| `FinalExecutionStatus::default()` | `FinalExecutionStatus::NotStarted` |

### RPC client

| 0.20.1 | This release |
|---|---|
| `RpcClient::call(method, params)` | `RpcClient::request(method, params)` |
| `RpcClient::genesis_config()` | `rpc.request::<_, serde_json::Value>("genesis_config", serde_json::json!([]))` |
| `RpcClient::block_effects(block)`, `RpcClient::maintenance_windows(id)`, `rpc::BlockEffects`, `rpc::StateChangeKindView`, `rpc::MaintenanceWindow` | `rpc.request::<_, serde_json::Value>("block_effects", serde_json::json!({ "block_id": hash }))` (or `{ "finality": "final" }`), `rpc.request::<_, serde_json::Value>("maintenance_windows", serde_json::json!({ "account_id": id }))`, or your own serde type in place of `Value` |
| `RpcClient::with_retry_config(url, c)` | `RpcClient::with_transport_and_retry_config(url, Arc::new(ReqwestTransport::default()), c)`, or `Near::custom(url, id).retry_config(c)` |
| `RpcClient::sandbox_fast_forward(n)`, `RpcClient::sandbox_patch_state(records)` | `sandbox.fast_forward(n)`, `sandbox.patch_state(records)`; for a sandbox node you started yourself, `near_kit_sandbox::fast_forward(&rpc, n)`, `near_kit_sandbox::patch_state(&rpc, records)` (sends the patch twice, as 0.20.1 did) |
| `TransportResponse { status, body }` (a struct literal in a custom `RpcTransport`) | `TransportResponse::new(status, body)`: the struct is `#[non_exhaustive]`; its fields stay readable |
| `RpcClient::DEFAULT_VIEW_STATE_PAGE_SIZE`, `RpcClient::DEFAULT_VIEW_ACCESS_KEYS_PAGE_SIZE` | Crate-private (10,000 and 100) |
| `rpc::RawTransactionResponse` (hidden) | `rpc::RpcTransactionResponse`, documented; `send_tx` and `tx_status` return it |

### Typed arguments

| 0.20.1 | This release |
|---|---|
| `protocol::TryIntoAccountId` (`"alice.near"`, `String`) | `impl Into<AccountId>` (`AccountId`, `&AccountId`, `&AccountIdRef`): `account!("alice.near")` for a literal, `"alice.near".parse::<AccountId>()?` at run time |
| `protocol::IntoNearToken` (`"5 NEAR"`) | `NearToken`: `NearToken::from_near(5)`, or `"5 NEAR".parse()?` |
| `protocol::IntoGas` (`"30 Tgas"`) | `Gas`: `Gas::from_tgas(30)`, or `"30 Tgas".parse()?` |
| `protocol::TryIntoGlobalContractId` | `impl Into<GlobalContractId>`: an `AccountId`, a `CryptoHash` or a `GlobalContractId` |
| `protocol::ParseGasError`, `protocol::ParseAmountError`, `Error::ParseGas`, `Error::ParseAmount` | Removed |

The same goes for every method that took one of these: the `Near` reads,
`FunctionCall::gas`/`deposit`, the signer constructors and loaders, the token
clients and near-kit-sandbox. `RpcClient` reads take `&AccountIdRef` and
`impl Into<BlockReference>` (see [Reads](#reads)).

### Contracts and tokens

| 0.20.1 | This release |
|---|---|
| `ContractClient`, `ContractClient::new(near, id)`, `Contract::Client: ContractClient` | `Contract::Client<C>`, `Contract::client(backend, id)` |
| `near_kit::{call, json, borsh}` attribute re-exports | Removed: `#[contract]` reads the markers itself |
| `standards::FungibleToken`, `standards::NonFungibleToken` (the clients) | `FungibleTokenClient<C = Near>`, `NonFungibleTokenClient<C = Near>`; `FungibleToken` and `NonFungibleToken` are now `Contract` markers |
| `Near::ft(id)`, `Near::nft(id)` | `near.contract::<FungibleToken>(id)`, `near.contract::<NonFungibleToken>(id)` |
| `FungibleToken::contract_id`, `is_registered`, `metadata`, `storage_balance_bounds`, `storage_balance_of`, `storage_deposit`, `transfer`, `transfer_call` | Same methods on `FungibleTokenClient<C>`; `transfer` takes a memo `Option` |
| `FungibleToken::transfer(to, amount)`, `transfer_call(to, amount, msg)` with `amount: impl Into<u128>` | `amount: u128`, so an integer literal needs no `_u128` suffix; pass a smaller integer type with `.into()` |
| `FungibleToken::balance_of`, `FungibleToken::total_supply` returning `FtAmount` | Same methods, returning `u128` |
| `FungibleToken::transfer_with_memo(to, amount, memo)`, `NonFungibleToken::transfer_with_memo(..)` | `transfer(to, amount, Some(memo))` |
| `NonFungibleToken::contract_id`, `metadata`, `token`, `tokens_for_owner`, `supply_for_owner`, `total_supply`, `transfer`, `transfer_call`, `transfer_with_approval` | Same methods on `NonFungibleTokenClient<C>`; `transfer` takes a memo `Option` |
| `FungibleToken::with_signer(s)`, `NonFungibleToken::with_signer(s)` | `near.with_signer(s).contract::<FungibleToken>(id)` |
| `FungibleToken::fmt`, `NonFungibleToken::fmt` (`Debug` on the clients) | `Debug` on `FungibleTokenClient`, `NonFungibleTokenClient` |
| `standards::FtAmount` (and `From<FtAmount> for u128`) | Removed: amounts are `u128`; format with `metadata().await?.decimals` |
| `standards::nep413::serialize_message(&params)` | `standards::nep413::hash_message(&params)` |
| `standards::nep413::NEP413_TAG` | Crate-private (2^31 + 413); `standards::nep413::hash_message(&params)` prepends it |
| `Viewer` and `TxSender` for `Box<T>` | `Arc<T>`, or the backend itself |

### Keys and signers

| 0.20.1 | This release |
|---|---|
| `InMemorySigner::new(id, "ed25519:...")?` | `InMemorySigner::new(id, "ed25519:...".parse()?)`: takes a `SecretKey`, can't fail |
| `InMemorySigner::from_secret_key(id, sk)?`, `InMemorySigner::from_parts(id, sk)` | `InMemorySigner::new(id, sk)` |
| `InMemorySigner::generate_implicit()` | `InMemorySigner::implicit(SecretKey::generate_ed25519())?` |
| `InMemorySigner::from_seed_phrase(id, phrase)`, `InMemorySigner::from_seed_phrase_with_path(id, phrase, path)` | `InMemorySigner::new(id, SecretKey::from_seed_phrase(KeyType::Ed25519, phrase, None, None)?)`, `Some(path)` for a path |
| `InMemorySigner::public_key()` (inherent, `&PublicKey`) | The `Signer` trait method (`PublicKey`); import `near_kit::signer::Signer` |
| `SecretKey::from_seed_phrase_with_path`, `SecretKey::from_seed_phrase_with_path_and_passphrase`, `SecretKey::ml_dsa65_from_seed_phrase`, `ml_dsa65_from_seed_phrase_with_path`, `ml_dsa65_from_seed_phrase_with_path_and_passphrase` | `SecretKey::from_seed_phrase(key_type, phrase, hd_path, passphrase)`, with `KeyType::Ed25519` or `KeyType::MlDsa65`; `None` for the default path or no passphrase |
| `SecretKey::generate_with_seed_phrase_words`, `generate_with_seed_phrase_custom`, `ml_dsa65_generate_with_seed_phrase`, `ml_dsa65_generate_with_seed_phrase_words`, `ml_dsa65_generate_with_seed_phrase_custom`, `signer::generate_seed_phrase` | `SecretKey::generate_with_seed_phrase(key_type, word_count)`; `None` words is 12 (Ed25519) or 24 (ML-DSA-65). For another path or a passphrase, derive again from the returned phrase |
| `signer::DEFAULT_HD_PATH`, `signer::DEFAULT_WORD_COUNT`, `signer::DEFAULT_ML_DSA_65_WORD_COUNT` | Removed; pass `None` |
| `RotatingSigner::from_key_strings(id, &keys)` | `RotatingSigner::new(id, keys.iter().map(|k| k.parse()).collect::<Result<_, _>>()?)` |
| `RotatingSigner::key_count()` | `public_keys().len()` |
| `RotatingSigner::signing_keys()` | Removed: `public_keys()` lists the keys, `into_per_key_signers()` gives a signer for each |
| `signer::EnvSigner` (`new()`, `from_env_vars(a, k)`) | `signer::from_env()`, `signer::from_env_vars(a, k)`, returning an `InMemorySigner` |
| `signer::FileSigner` (`new(network, id)`, `from_file(path, id)`) | `signer::from_credentials(network, id)`, `signer::from_credentials_file(path, id)` |
| `signer::KeyringSigner::new(network, id, "ed25519:...")` | `signer::from_keyring(network, id, &public_key)` with a parsed `PublicKey` |
| `.into_inner()` on those signers | Not needed: the loaders return the `InMemorySigner` |
| `PublicKeyHandle::full_pubkey()` | `PublicKeyHandle::as_full()` |
| `signer::SignerError` (exhaustive) | `#[non_exhaustive]`, with new `InvalidKey`, `NoKeys` and `MixedAccounts` variants |
| `PublicKey::ml_dsa65_from_bytes(bytes)` | `PublicKey::MlDsa65(bytes)` |
| `Signature::ed25519_from_bytes(b)`, `Signature::secp256k1_from_bytes(b)`, `Signature::ml_dsa65_from_bytes(b)` | `Signature::Ed25519(b)`, `Signature::Secp256k1(b)`, `Signature::MlDsa65(b)` |
| `KeyType::try_from(u8)` | Match the byte yourself; `key_type as u8` gives the tag |
| `KeyType::public_key_len()`, `KeyType::signature_len()` | Crate-private: public keys are 32 (Ed25519), 64 (secp256k1) and 1952 (ML-DSA-65) bytes, signatures 64, 65 and 3309 |
| `signer::ML_DSA_65_PUBLIC_KEY_LENGTH`, `ML_DSA_65_SEED_LENGTH`, `ML_DSA_65_SECRET_KEY_LENGTH`, `ML_DSA_65_SIGNATURE_LENGTH`, `ML_DSA_65_HASH_LENGTH` | Crate-private (1952, 32, 4032, 3309 and 32); the array types of `PublicKey::MlDsa65`, `PublicKeyHandle::MlDsa65Hash`, `Signature::MlDsa65`, `SecretKey::ml_dsa65_from_seed` and `SecretKey::to_ml_dsa65_expanded_bytes` carry them |

### Error variants

| 0.20.1 | This release |
|---|---|
| `Error::NoSigner` | Removed: sending from a `Near` is a compile error |
| `Error`, `KeyStoreError` (exhaustive) | `#[non_exhaustive]`: a `match` on either needs a `_` arm |
| `Error::InvalidTransaction(String)` | `Error::Validation(String)` for local checks before sending; `Error::Outcome(OutcomeError)` for an outcome with no usable result; `Error::Decode(DecodeTransactionError)` for a signed transaction that fails to decode |
| `Error::DelegateDecode` | `Error::DecodeDelegate`; `?` on `SignedDelegateAction::from_base64` converts into it |
| `Error::InvalidTransaction` text for a local check: `Invalid transaction: …` | `Error::Validation` text: `Refused before signing: …` |
| `TxExecutionError` text (also `failure_message()`): `ActionError: action #0: …`, `InvalidTxError: …` | `Action #0: …`, and the `InvalidTxError` text alone, as nearcore prints them |
| `FinalExecutionOutcome::result()` text for a failed action: `Invalid transaction: Action error: action #0: …` | `OutcomeError::ActionFailed` text: `Action #0: …` |
| `KeyStoreError::Io`, `Json`, `PathError` | Removed: nothing returned them |
| `Error::is_invalid_tx()` | `matches!(e, Error::InvalidTx(_))` |
| `RpcError::is_account_not_found()`, `is_contract_not_deployed()`, `is_method_not_found()`, `is_contract_panic()`, `is_global_contract_not_found()` | Match the variant: `matches!(e, RpcError::AccountNotFound { .. })` and so on |
| `From<Infallible> for Error` | Removed |
| `RpcError::UnknownBlock`, `UnknownChunk`, `UnknownEpoch`, `UnknownReceipt` | `RpcError::BlockNotFound`, `ChunkNotFound`, `EpochNotFound`, `ReceiptNotFound` |
| `RpcError::RequestTimeout { .. }` | `RpcError::Timeout { .. }` |
| `RpcError::Timeout(u32)` | Removed: nothing returned it |
| `RpcError::InvalidTransaction { message, details }` | `RpcError::InvalidTxUnparsed { message, details }` |
| `RpcError::network(..)`, `RpcError::invalid_transaction(..)` | Crate-private; build the variant (`RpcError::Network { .. }`) in a custom transport |
| `ActionViewConversionError::InvalidBase64 { field, source: base64::DecodeError }` | `InvalidBase64 { field, message: String }`, with the same `Display` text |
| `protocol::UniversalStateInitError` | `protocol::StateInitError` (from near-global-contracts): same `Malformed` and `NonCanonical` variants, different `Display` text |

### Protocol types

| 0.20.1 | This release |
|---|---|
| `protocol::AccountIdRef` | `AccountIdRef` (crate root) |
| `protocol::RawStateInit` (near-kit's own) | Same path, now near-global-contracts' `RawStateInit` (base64 in JSON): `as_bytes()` → `as_ref()`, `into_bytes()` → `Vec::from(raw)`, `derive_account_id()` → `try_derive_account_id()?` |
| `protocol::StateInitExt` (`StateInit::by_hash(hash, data)`, `StateInit::by_publisher(id, data)`) | `StateInitV1::code(hash_or_id).with_data_entry(k, v).into()`, or `StateInit::V1(StateInitV1 { code: hash_or_id.into(), data })` |
| `protocol::MAX_NONCES_FOR_GAS_KEY` | `AccessKeyPermission::MAX_NONCES_FOR_GAS_KEY` |
| `CryptoHash::to_vec()`, `From<&CryptoHash> for [u8; 32]` | `hash.as_bytes().to_vec()`; `<[u8; 32]>::from(hash)` (`CryptoHash` is `Copy`) |
| `ChainId::default()` (mainnet) | `ChainId::mainnet()` |
| `ChainId::from_str(s)` (`s.parse::<ChainId>()`), `AsRef<str> for ChainId` | `ChainId::from(s)` or `ChainId::new(s)`; `chain_id.as_str()` |
| `AccessKeyPermission::function_call(rx, methods, allowance)`, `AccessKeyPermission::full_access()`, `AccessKey::function_call(..)` | `AccessKeyPermission::FunctionCall(FunctionCallPermission { allowance, receiver_id, method_names })`, `AccessKeyPermission::FullAccess`, `AccessKey { nonce: 0, permission }`; or `Action::add_function_call_key(..)` |
| `VersionedDelegateActionPayload::get_actions()` | `let VersionedDelegateActionPayload::V2(d) = payload;` then `d.actions.iter().cloned().map(Action::from)` |
| `NonDelegateAction::into_inner()`, `NonDelegateAction::try_from(action)` (`Error = ()`) | `Action::from(non_delegate)`; `NonDelegateAction::from_action(action)`, which returns an `Option` |
| `UniversalStateInit::from_raw_lenient(raw)` | `UniversalStateInit::from_raw(raw)`, which rejects a non-canonical encoding as nearcore does; to inspect such bytes, `borsh::from_slice::<UniversalStateInit>(raw.as_ref())` |
| `Transaction::get_hash_and_size()` | `tx.get_hash()`; `borsh::to_vec(&tx)?.len()` for the size |

### Sandbox (near-kit-sandbox)

| 0.20.1 | This release |
|---|---|
| `SandboxConfig::shared()`, `SandboxConfig::fresh()`, `SandboxConfig::builder()` | `Sandbox::builder().shared()`, `Sandbox::builder().fresh()`, `Sandbox::builder()` |
| `Sandbox::client() -> Near` | `sandbox.root_account() -> SigningNear` |
| `Sandbox::rpc_url()` | `sandbox.root_account().near().rpc().url()` |
| `Sandbox::root_account_id()` | `sandbox.root_account().account_id()` |
| `Sandbox::chain_id()` | `sandbox.root_account().near().chain_id()` |
| `Sandbox::root_signer()` | `sandbox.root_account().signer().clone()` |
| `SandboxError::InvalidRootAccount`, `SandboxError::InvalidAccount` | Removed: account IDs are typed |
| `SandboxError::RootSigner` | Nothing; the root key is a valid constant |
| `SandboxError` (exhaustive) | `#[non_exhaustive]`: a `match` needs a `_` arm |
| `SandboxBuilder::root_account("x")`, `Sandbox::set_balance("x", amount)` | Take `impl Into<AccountId>`: `account!("x")` |
| `SANDBOX_ROOT_ACCOUNT: &str` | `&AccountIdRef` |

## Reads

`Near` keeps the reads that do more than forward one RPC call: `view`,
`balance`, `account_exists` and `tx_status`. For everything else, call the
`RpcClient` method through `near.rpc()`. Reads still default to
`BlockReference::default()`, which is `Final`, and every read builder picks a
block with one method, `.at(..)`.

`RpcClient` reads take the account as `&AccountIdRef` (so `&AccountId` and
`&account!(..)` both work) and the block as `impl Into<BlockReference>`. Pass
the value itself, such as `Finality::Final`. Writing `.into()` or
`Default::default()` there no longer compiles: with a generic parameter rustc
can't infer the target and reports "type annotations needed".

`near.view(..)` now returns a `View<Near, T>` whose error is
`ContractError<RpcError>`, not `Error`. `?` into `near_kit::Error` still works.
Code that matched on `Error` should match `ContractError::Backend(rpc_error)`
and `ContractError::Codec(codec_error)`, or call `.map_err(Error::from)` first.

```rust
// `height` is a recent block height: the default endpoints aren't archival.
let height = near.rpc().block(Finality::Final).await?.header.height;

// 0.20.1
let count: u64 = near.view("counter.testnet", "get_count").at_block(height).await?;
let read = near.view::<u64>("counter.testnet", "get_count").with_metadata().await?;
let account = near.account("alice.testnet").finality(Finality::Optimistic).await?;

// This release
let count: u64 = near.view(account!("counter.testnet"), "get_count").at(height).await?;
let read = near.view::<u64>(account!("counter.testnet"), "get_count").with_metadata().await?;
println!("{} at block {}", read.result, read.block_height);
let account = near
    .rpc()
    .view_account(&account!("alice.testnet"), Finality::Optimistic)
    .await?;
```

If your error enum wraps `near_kit::Error` with `#[from]`, a plain `?` on a
view no longer reaches it: `ContractError<RpcError>` to `Error` to your enum
is two conversions, and `?` does one. Convert first, or add the second `From`
once:

```rust
// MyError has a `Near(#[from] near_kit::Error)` variant.
let count: u64 = near
    .view(account!("counter.testnet"), "get_count")
    .await
    .map_err(near_kit::Error::from)?;

// Or, once, so a plain `?` works:
impl From<ContractError<RpcError>> for MyError {
    fn from(e: ContractError<RpcError>) -> Self {
        MyError::Near(e.into())
    }
}
```

## Signing and sending

### A separate signing client

`near.with_signer(signer)` returns a `SigningNear`. It has no `Deref` to
`Near`: read through `alice.near()`. Typed contract views and token views
work on a `SigningNear` directly. `SigningNear::from_env()` replaces
`Near::from_env()` and fails unless both `NEAR_ACCOUNT_ID` and
`NEAR_PRIVATE_KEY` are set; build a `Near` for read-only use. When
`NEAR_NETWORK` is an RPC URL it also fails without `NEAR_CHAIN_ID` (0.20.1 used
the chain ID `"custom"`).

```rust
// 0.20.1
let near = Near::testnet().credentials("ed25519:...", "alice.testnet")?.build();
let balance = near.balance("alice.testnet").await?;

// This release
let near = Near::testnet().build();
let alice = near.with_signer(InMemorySigner::new(account!("alice.testnet"), "ed25519:...".parse()?));
let balance = alice.near().balance(alice.account_id()).await?;
```

### Transactions are a receiver plus actions

`SigningNear::transaction(receiver)` returns a `Tx`. `Tx::add_action` takes
anything that converts into an `Action`, including a `FunctionCall`, and
`.await` signs and sends. An argument-encoding error is returned from
`.await`, as before. The receiver is never filled in for you: a state init
sent to any account but the one it derives, or a delegate action sent to
anyone but its sender, fails with `Error::Validation` before signing.

```rust
// 0.20.1
near.transaction("sub.alice.testnet")
    .create_account()
    .transfer(NearToken::from_near(5))
    .add_full_access_key(key)
    .deploy(wasm)
    .call("new")
        .args(serde_json::json!({ "owner_id": "alice.testnet" }))
    .send()
    .await?;

// This release
use near_kit::protocol::Action;

alice
    .transaction(account!("sub.alice.testnet"))
    .add_action(Action::create_account())
    .add_action(Action::transfer(NearToken::from_near(5)))
    .add_action(Action::add_full_access_key(key))
    .add_action(Action::deploy_contract(wasm))
    .add_action(FunctionCall::new("new").args(serde_json::json!({ "owner_id": "alice.testnet" })))
    .await?
    .result()?;
```

A failed action is still `Ok(outcome)` with `outcome.is_failure()`; `Err`
means the transaction didn't run, or (after a timeout or a dropped
connection) that near-kit couldn't find out whether it did. `FinalExecutionOutcome` is now
`#[must_use]`, so a send whose outcome is dropped (`.await?;`) warns: check
`is_failure()` or `receipt_failures()`, or call `.result()?` to turn a failed
action into an `Err`. For an FT or NFT `transfer_call`, `.result()?` isn't
enough: when the receiver's `ft_on_transfer` or `nft_on_transfer` fails, the
token's resolve callback handles it and tries to refund the sender, so the
transaction succeeds. The refund can fall short (the receiver may have moved
the tokens already), so check the returned amount (or `bool` for NFTs), and
`receipt_failures()` for why the receiver failed.

### Wait levels: `Final` by default

The wait level is now part of the signing client's type, set once with
`.wait_until::<W>()`. `SigningNear` with no parameter is `SigningNear<Final>`.
**The default changed from `ExecutedOptimistic` to `Final`**, for sends and for
`near.send_signed(..)`. This is not a compile error. Sends take longer, and a
default (final) read afterwards sees the write. To keep the 0.20.1 behaviour,
use `.wait_until::<ExecutedOptimistic>()`. See
[finality](https://docs.rs/near-kit/latest/near_kit/guide/finality/index.html).

```rust
// 0.20.1
let response = near
    .transfer("bob.testnet", NearToken::from_near(1))
    .send()
    .wait_until::<Included>()
    .await?;

// This release
let response = alice
    .wait_until::<Included>()
    .transaction(account!("bob.testnet"))
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?;
```

A type of yours that held a `Near` to both view and send should hold a
`SigningNear<W>`, which is both `Viewer` and `TxSender`. If some instances
only ever viewed (built with no signer), give those a `Near` instead: it's
`Viewer` only, has no wait level, and reads at whatever block you pass. Split
your impls the same way: view-only ones for `MyWrapper<Near>`, and viewing
and sending ones for `MyWrapper<SigningNear<W>>`.

### Nonces

See [nonces](https://docs.rs/near-kit/latest/near_kit/guide/nonces/index.html)
for the full picture. What changed:

- **Cached nonces are shared per chain.** The cache is keyed by the chain's
  genesis hash, account and public key, not by RPC URL: each client asks its
  node for the genesis hash (`status`) before its first nonce. Every client
  for one network shares one sequence whatever its endpoint or chain ID, and
  two networks with one chain ID (two sandboxes) don't. A backend that
  answers `status` with "method not found" (code -32601) shares nonces by
  chain ID (`Near::custom`'s second argument), so keep passing the network's
  real one; any other `status` failure fails the send. A test transport that
  panics on methods it doesn't expect needs a `status` reply.
- **More rejections are retried.** `max_nonce_retries` (on `NearBuilder` for
  every `SigningNear` made from that `Near`, or on one `SigningNear`) covers
  `InvalidNonce`, `NonceTooLarge` and `Expired`. A `NonceTooLarge` rejection
  drops the cached nonce and retries from a fresh view. After `InvalidNonce`,
  the retry uses the higher of the reported nonce and the fresh access key
  view.
- **A universal account bootstrap is signed with `bootstrap_nonce + 1`** on
  every attempt, read from the account view rather than the cache.

### Helpers over a signing client

Extension traits implemented for `Near` move to `SigningNear`:

```rust
// 0.20.1
impl Wrap for Near {
    async fn wrap(&self, amount: NearToken) -> Result<(), Error> {
        self.call("wrap.testnet", "near_deposit").deposit(amount).await?;
        Ok(())
    }
}

// This release
impl Wrap for SigningNear {
    async fn wrap(&self, amount: NearToken) -> Result<(), Error> {
        self.transaction(account!("wrap.testnet"))
            .add_action(FunctionCall::new("near_deposit").deposit(amount))
            .await?
            .result()?;
        Ok(())
    }
}
```

## Contracts

### Typed contracts

`#[near_kit::contract]` generates `CounterClient<C = Near>`. Views need
`C: Viewer + Clone` and return a `View`; `#[call]` methods need
`C: TxSender + Clone` and return a `Call`. Both are at the crate root and take
`.at(..)` or `.gas(..)`/`.deposit(..)` before `.await`. Their error is
`ContractError<C::ViewError>` or `ContractError<C::SendError>`.
`near.contract::<Counter>(id)` gives a view-only client; build it from a
`SigningNear` to call. A call resolves to the backend's output, `W::Response`
for `SigningNear<W>` (a `FinalExecutionOutcome` at the default `Final`). For
another wait level, build the client from a re-levelled sender:
`alice.wait_until::<Included>().contract::<Counter>(id)`.

```rust
// 0.20.1
let counter = near.contract::<Counter>("counter.testnet".parse::<AccountId>()?);
counter.add(AddArgs { value: 1 }).await?;

// This release
let counter = alice.contract::<Counter>(account!("counter.testnet"));
counter.add(AddArgs { value: 1 }).gas(Gas::from_tgas(10)).await?.result()?;
let count = counter.get_count().at(Finality::Optimistic).await?;
```

The macro now rejects declarations it used to compile into something wrong:

| 0.20.1 | This release |
|---|---|
| `#[call] fn m(&mut self) -> T` (return type ignored) | A compile error; decode the outcome yourself |
| Methods named `contract_id`, `client` or `clone`, including as raw identifiers | A compile error; rename them |
| An argument bound to `_` or a pattern (dropped: the method sent `{}`) | A compile error; name the argument (`args: Args`) |
| Generic methods, lifetime parameters, `async fn` | A compile error; use `impl Trait` or a concrete type, drop `async` |
| `#[cfg]` / `#[cfg_attr]` on a method (ignored) | Applied to the generated client method and `FunctionCall` constructor; `#[call]`, `#[json]` or `#[borsh]` behind `#[cfg_attr]` is a compile error (declare the method twice, each under `#[cfg]`) |
| `fn r#type(&self)` sent the method name `r#type` | It sends `type` |

### Your own backend

Implement `Viewer`, `TxSender` or both, and typed contract clients, `View`,
`Call` and the token clients run on your type. Build the view result with
`ViewFunctionResult::new(bytes, block_height, block_hash)`. Both traits are
implemented for `&T` and `Arc<T>` (with `T: ?Sized`), so
`Arc<dyn MySender>` works once you implement `TxSender for dyn MySender`.
Awaiting a `View` or `Call` needs a `'static` backend; through a shorter
`&T` borrow, use `.with_metadata()` on a view and `.send()` on a call.

### Fungible and non-fungible tokens

FT and NFT clients enter through the same door as any contract:
`near.contract::<FungibleToken>(id)` (import the markers from
`near_kit::standards`). `FungibleToken::client(backend, id)` builds one over
any backend; clones of a client share its metadata cache. `balance_of` and
`total_supply` return `u128` in the token's smallest unit and no longer read
the metadata first. A non-numeric `U128`/`U64` string from a token contract is
now `ContractError::Codec`, not an RPC error. See
[contracts and tokens](https://docs.rs/near-kit/latest/near_kit/guide/contracts_and_tokens/index.html).

```rust
// 0.20.1
near.ft("wrap.near".parse::<AccountId>()?)
    .transfer_with_memo("bob.near", 1_000_u128, "rent")
    .await?;

// This release
use near_kit::standards::FungibleToken;

alice
    .contract::<FungibleToken>(account!("wrap.near"))
    .transfer(account!("bob.near"), 1_000, Some("rent".to_owned()))
    .await?
    .result()?;
```

## Relayers and meta-transactions

`sign_delegate` replaces the builder's `delegate` and returns the
`SignedDelegateAction` itself. The relayer sends it to the delegate's sender;
any other receiver fails with `Error::Validation` before signing (nearcore
would reject it on chain, at the relayer's cost). See
[meta-transactions](https://docs.rs/near-kit/latest/near_kit/guide/meta_transactions/index.html).

```rust
// 0.20.1
let result = near
    .transaction("guestbook.testnet")
    .call("add_message")
        .args(serde_json::json!({ "text": "hi" }))
    .finish()
    .delegate(DelegateOptions::with_offset(200))
    .await?;
let payload = result.payload;

// This release: the user signs
let call = FunctionCall::new("add_message").args(serde_json::json!({ "text": "hi" }));
let signed = alice
    .sign_delegate(
        account!("guestbook.testnet"),
        vec![call.try_into()?],
        DelegateOptions { block_height_offset: Some(200), ..Default::default() },
    )
    .await?;
let payload = signed.to_base64();

// The relayer decodes and sends. A bad payload is `Error::DecodeDelegate`.
let signed = SignedDelegateAction::from_base64(&payload)?;
let outcome = relayer
    .transaction(signed.sender_id())
    .add_action(Action::delegate(signed))
    .await?;
// The relayer's transaction succeeds even when the user's call fails.
if let Some(error) = outcome.receipt_failures().next() {
    println!("inner action failed: {error}");
}
```

Behaviour changes:

- **Delegate nonces come from the shared sequence.** Without
  `DelegateOptions::nonce`, `sign_delegate` takes the next nonce from the same
  cache `sign` and sends use (0.20.1 used the access key's nonce + 1). Two
  delegate actions signed before relaying, or one signed while a transaction
  from the same key is pending, no longer share a nonce. An explicit nonce
  still wins.
- **Retry policy belongs to the sender.** A relayer sets
  `relayer.clone().max_nonce_retries(n)` once instead of per transaction.
- **New relayer helpers.** `SignedDelegateAction::verify()` checks the
  signature against the delegate action's public key.
  `FinalExecutionOutcome::receipt_failures()` yields every failed receipt's
  `ActionError` in RPC order: for a relayed transaction `is_failure()` and
  `result()` can both look successful while the user's inner actions failed,
  and only `receipt_failures()` shows it.
- **A delegate's inner actions are `Action`s.** `DelegateActionView::actions`
  and `DelegateActionV2View::actions` are `Vec<Action>` (were
  `Vec<ActionView>`), because the node sends them in the protocol `Action`
  JSON. A relayed state init or `UseGlobalContract` used to be reported as
  "no execution outcome" although it ran; it now decodes. An inner
  `DeployContract` or `DeployGlobalContract` carries the WASM, not a hash.
- **`Action` has serde.** `Action` and its payload types implement
  `Serialize`/`Deserialize` in nearcore's JSON shape. Byte fields such as
  `DeployGlobalContractAction::code` are base64 strings in JSON (were arrays of
  numbers). `NonDelegateAction` rejects a nested delegate when deserializing.

## Keys and signers

The loader types are now functions; the tables above map each one. See
[keys](https://docs.rs/near-kit/latest/near_kit/guide/keys/index.html).

Key parsing and printing changed too, mostly without a compile error:

- **Ed25519 secret keys print in the 64-byte form.** `to_string()`,
  `Display` and serde give `ed25519:<base58(seed ‖ public key)>`, the form
  nearcore, near-cli-rs and neard key files read; 0.20.1 printed the 32-byte
  seed, which they reject. Both forms still parse, and `as_bytes()` still
  returns the 32-byte seed. Code that compares a key's string or JSON with a
  stored 32-byte form needs updating.
- **A 64-byte Ed25519 key is checked.** One whose public half isn't the one
  its seed derives fails to parse with the new
  `ParseKeyError::KeyPairMismatch`; 0.20.1 ignored the public half.
  Exhaustive matches on `ParseKeyError` need an arm for it.
- **More key forms load.** Secp256k1 parsing accepts near-api-js's 96-byte
  `secp256k1:<base58(secret ‖ public key)>` form (`KeyPairMismatch` if the
  halves disagree) and still prints the 32-byte secret. ML-DSA-65 parsing, and
  so `from_credentials`, `from_credentials_file`, `from_keyring` and
  `from_env`, accepts the 4032-byte expanded key nearcore and near-cli-rs
  write, as well as the 32-byte seed; 0.20.1 returned
  `ParseKeyError::InvalidLength`. The seed can't be recovered from an expanded
  key, so its `as_bytes()` and `to_string()` are the 4032-byte form. A
  malformed expanded key returns `InvalidFormat`, one whose parts don't belong
  to one key `KeyPairMismatch`.
- **Credentials files are checked.** `from_credentials` and
  `from_credentials_file` return `Error::Config` for a file whose
  `public_key` names a different key than its `private_key`.
- **Keyring lookups find near-cli-rs entries.** `from_keyring` looks up an
  ML-DSA-65 entry by its `ml-dsa-65-hash:` handle first (as near-cli-rs 0.31
  stores it), then by the full key.
- **NEP-413 messages from any key type read back.** `SignedMessage` and
  `AuthPayload` JSON signed with a secp256k1 or ML-DSA-65 key, and the
  prefixed signature near-cli-rs prints, now deserialize.

## Errors

Each layer returns its own error, and each converts into `near_kit::Error`
with `?`. See [errors](https://docs.rs/near-kit/latest/near_kit/guide/errors/index.html).

| API | Now returns |
|---|---|
| `SignedTransaction(V1)::from_bytes` / `from_base64` | `DecodeTransactionError` (becomes `Error::Decode`) |
| `SignedDelegateAction::from_bytes` / `from_base64`, `VersionedSignedDelegateAction::from_bytes` / `from_base64` | `DecodeDelegateError` (becomes `Error::DecodeDelegate`); `from_bytes` returned `borsh::io::Error` |
| `FinalExecutionOutcome::result` / `json` | `OutcomeError` (becomes `Error::InvalidTx`, `Error::Json` or `Error::Outcome`) |
| `FunctionCall::into_action`, `TryFrom<FunctionCall> for Action` | `CodecError` |
| `InMemorySigner::implicit`, `SecretKey::from_seed_phrase` / `generate_with_seed_phrase`, `RotatingSigner::new` / `from_signers` | `SignerError` |
| Views and calls (`View`, `Call`, typed clients, token clients) | `ContractError<E>`: `Codec(CodecError)` or `Backend(E)` |

Two patterns need a change. A variant constructor passed to `map_err` needs
the conversion first: `.map_err(|e| MyError::Signer(e.into()))?`. A direct
match on the result matches the new type's variants (for example
`OutcomeError::InvalidTx` instead of `Error::InvalidTx`), or calls
`.map_err(near_kit::Error::from)` first. `ContractError<E>` implements
`std::error::Error` whenever `E` is `Debug + Display`, so `?` into `anyhow`
works.

### RPC errors and retries

- **Unknown transactions and receipt lookups have typed errors.**
  `RpcError::TransactionNotFound(hash)` (from `tx_status` at the default
  `Submitted` level), `RpcError::ShardNotTracked` and
  `RpcError::ReceiptToTx(ReceiptToTxError)` used to be a generic
  `RpcError::Rpc` with code `-32000` that was retried (about 3.5 s with the
  default `RetryConfig`) and reported `is_retryable() == true`. They are
  returned after one attempt and aren't retryable.
- **`RpcError::Rpc` has a `cause` field**, the node's error cause name when
  the response has one. A `-32000` server error is retried (and
  `is_retryable()`) only when it has no cause or one that clears with time
  (`NODE_IS_SYNCING`, `NO_NEW_BLOCKS`, `BUSY`, `NOT_CONFIRMED`,
  `CHUNK_NOT_CERTIFIED`); 0.20.1 retried every `-32000` error. Struct patterns
  and literals without `..` need the new field.

```rust
// 0.20.1
match err {
    RpcError::UnknownBlock(_) => { /* ... */ }
    RpcError::Rpc { code, message, .. } => println!("{code}: {message}"),
    _ => {}
}

// This release
match err {
    RpcError::BlockNotFound(_) => { /* ... */ }
    RpcError::TransactionNotFound(hash) => { /* ... */ }
    RpcError::Rpc { code, cause: Some(cause), .. } => println!("{code}: {cause}"),
    _ => {}
}
```

### Transaction outcomes

- **`ExecutionStatus::InvalidTx(InvalidTxError)` is new.** A transaction
  that was included in a chunk but rejected when the chunk was applied has an
  outcome with this status. It used to fail to decode and be reported as "no
  execution outcome"; sends now return `Error::InvalidTx` (and
  `FinalExecutionOutcome::result` returns `OutcomeError::InvalidTx`).
  Exhaustive matches on `ExecutionStatus` need an arm for it. `is_failure()`
  is true for it; `failure()` still returns only an `ActionError`.
- **An outcome that fails to decode is an error.** It used to read as
  `outcome: None`, surfacing as "no execution outcome" or as a
  `SendTxResponse` without an outcome. `RpcClient::send_tx`/`tx_status` now
  return `RpcError::Json`, and sends and `Near::tx_status` return it as
  `Error::Rpc`. `outcome` is `None` only when the node sent no outcome.

## Sandbox

`sandbox.patch_state(records)` replaces the `RpcClient` method. It still
sends the patch twice but no longer sleeps 100 ms afterwards, so poll for the
state you wrote, or use `sandbox.set_balance(..)`, which does. See
[testing](https://docs.rs/near-kit/latest/near_kit/guide/testing/index.html).

## Platforms and features

The feature names are unchanged. See
[platforms](https://docs.rs/near-kit/latest/near_kit/guide/platforms/index.html).

- **`near-kit-primitives`** holds the protocol types, signers, `Viewer` and
  `TxSender`, and (with its `contracts` feature) `#[contract]`. It has no RPC
  client or HTTP stack. near-kit re-exports it, so
  depend on it directly only if you want the types without the client, such as
  in a contract crate.
- **`contracts` without `rpc`** now generates the backend-generic
  `CounterClient<C>` as well as the offline `FunctionCall` constructors. With
  no `Near` to default to, name the backend: `CounterClient<MyBackend>`.
- **Write `MaybeSend` and `MaybeSync`** (crate root) instead of `Send` and
  `Sync` in bounds that forward to `Viewer` or `TxSender`. They mean `Send` and
  `Sync` everywhere except `wasm32-unknown-unknown`, where they require
  nothing, so the same code builds there.
- **Derive `0s` ids through near-kit.** near-global-contracts'
  `StateInit::derive_account_id` calls NEAR host functions when any crate in
  your workspace enables `near-sdk/unit-testing` (feature unification turns
  it on for every crate), and a test binary that links near-kit but not
  near-sdk then fails to link (`_keccak256` undefined). near-kit's
  `DeterministicStateInitAction::derive_account_id`, which sends use, hashes in
  pure Rust and gives the same id.
- **`near-kit-macros` changed incompatibly** (`Client<C>`, reserved method
  names, the removed standalone `call`/`json`/`borsh` markers). Depend on
  near-kit or near-kit-primitives with `contracts` rather than on
  `near-kit-macros` directly.

## Upgrading from older versions

To upgrade from a release before 0.20.1, start with the
[MIGRATION.md at the `near-kit-v0.20.1` tag](https://github.com/r-near/near-kit-rs/blob/near-kit-v0.20.1/MIGRATION.md),
which covers the 0.18 and 0.19 upgrades, and the 0.20.0 entries in
[near-kit's CHANGELOG](crates/near-kit/CHANGELOG.md). Then follow this guide.
