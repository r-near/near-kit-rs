# Keys and signers

Read this when you load, generate, store or convert keys: which string
forms near-kit reads and writes, how to load a key from the environment, a
near-cli credentials file, the system keyring or a seed phrase, and how to
sign with a key near-kit never sees. A [`SecretKey`](crate::signer::SecretKey)
is a key; a [`Signer`](crate::signer::Signer) is an account plus the key or
keys it signs with, and is what
[`Near::with_signer`](crate::Near::with_signer) takes.

## Key types and string forms

NEAR keys print as `<type>:<base58 bytes>`. near-kit supports three types:

| [`KeyType`](crate::signer::KeyType) | Public key | Secret key parses from | Secret key prints as |
|---|---|---|---|
| `ed25519` | 32 bytes | 32-byte seed, or 64-byte seed ‖ public key | 64 bytes |
| `secp256k1` | 64 bytes (x ‖ y, no prefix) | 32-byte secret, or 96-byte secret ‖ public key | 32 bytes |
| `ml-dsa-65` | 1952 bytes | 32-byte seed, or 4032-byte expanded key | the form it was parsed from |

A few consequences:

- **Ed25519** keys print in the 64-byte form that near-cli, near-api-js and
  wallets write, so a printed key can be pasted into any of them. The
  shorter 32-byte form parses too.
- **Secp256k1** keys from near-api-js carry the public key after the
  secret (96 bytes). They parse, and print back as the 32-byte secret.
- **ML-DSA-65** (post-quantum) keys are normally the 32-byte FIPS-204 seed.
  nearcore-based tools (near-cli-rs, `near-crypto`) write the 4032-byte
  expanded form, which parses too. The seed can't be recovered from it, so
  such a key keeps and prints the expanded form.
  [`to_ml_dsa65_expanded_bytes`](crate::signer::SecretKey::to_ml_dsa65_expanded_bytes)
  exports the expanded form of any ML-DSA-65 key for those tools.

```rust
use near_kit::signer::{KeyType, PublicKey, SecretKey};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let alice = SecretKey::generate_ed25519();
let printed = alice.to_string();
assert!(printed.starts_with("ed25519:"));
# assert_eq!(bs58::decode(&printed["ed25519:".len()..]).into_vec()?.len(), 64);

// Round trip, and the public key that goes into AddKey.
let parsed: SecretKey = printed.parse()?;
assert_eq!(parsed.public_key(), alice.public_key());
let public: PublicKey = alice.public_key().to_string().parse()?;
assert_eq!(public.key_type(), KeyType::Ed25519);

// Debug output never shows the secret; Display does.
assert_eq!(format!("{alice:?}"), "SecretKey(ed25519:***)");

// ML-DSA-65: a seed key, and its expanded form for nearcore-based tools.
let quantum = SecretKey::generate_ml_dsa65();
assert_eq!(quantum.as_bytes().len(), 32);
let expanded = quantum.to_ml_dsa65_expanded_bytes().expect("an ML-DSA-65 key");
assert_eq!(expanded.len(), 4032);
# Ok(())
# }
```

Log a key with `{:?}`, never `{}`: [`Display`](std::fmt::Display) prints
the whole secret, which is what writing it to a credentials file needs.

### Errors when parsing

A string that doesn't parse returns a
[`ParseKeyError`](crate::signer::ParseKeyError) (inside `Error::ParseKey`
when it comes through a loader). The variants say what was wrong: an
unknown type prefix, bad base58, the wrong number of bytes, or a secp256k1
secret outside the curve's range.

The forms that carry more than the secret are checked for consistency. A
64-byte ed25519 or 96-byte secp256k1 key whose public half isn't the one
the secret derives, or an expanded ML-DSA-65 key whose parts don't belong
together, fails with
[`ParseKeyError::KeyPairMismatch`](crate::signer::ParseKeyError::KeyPairMismatch).
That usually means two keys were spliced together by hand or a file was
corrupted, and signing with the secret would produce signatures for a
different public key than the one shown:

```rust
use near_kit::signer::{ParseKeyError, SecretKey};

# fn main() -> Result<(), Box<dyn std::error::Error>> {
# let (alice, bob) = (SecretKey::generate_ed25519(), SecretKey::generate_ed25519());
# let mut bytes = alice.as_bytes().to_vec();
# bytes.extend_from_slice(bob.public_key().as_bytes());
// Alice's secret followed by Bob's public key.
let spliced = format!("ed25519:{}", bs58::encode(bytes).into_string());
assert_eq!(spliced.parse::<SecretKey>().unwrap_err(), ParseKeyError::KeyPairMismatch);
# Ok(())
# }
```

Public keys are checked for type and length only, as nearcore does, so any
key the chain returns decodes and re-encodes unchanged. To reject an
ed25519 or secp256k1 key that isn't a valid curve point, call
[`PublicKey::validate`](crate::signer::PublicKey::validate). An invalid
key never verifies a signature either way.

### ML-DSA-65 keys on chain

An ML-DSA-65 public key is 1952 bytes, so the chain stores the access key
under a 32-byte hash of it. `view_access_key_list` returns that hash as
`ml-dsa-65-hash:<base58>`. It is a
[`PublicKeyHandle`](crate::signer::PublicKeyHandle), not a `PublicKey`:
parsing it as a `PublicKey` fails with `ParseKeyError::MlDsa65HashHandle`.
Signing and `AddKey` need the full key; to find a full key in a key list,
compare handles:

```rust
use near_kit::signer::{PublicKeyHandle, SecretKey};

let key = SecretKey::generate_ml_dsa65().public_key();
let handle = key.to_ml_dsa65_hash().expect("an ML-DSA-65 key");
assert!(handle.to_string().starts_with("ml-dsa-65-hash:"));
assert!(handle.refers_to(&key));

// Ed25519 and secp256k1 keys are listed in full.
let ed: PublicKeyHandle = SecretKey::generate_ed25519().public_key().into();
assert!(ed.as_full().is_some());
```

## Loading a signer

Every loader reads the key once and returns a plain
[`InMemorySigner`](crate::signer::InMemorySigner).

### From the environment

[`signer::from_env`](crate::signer::from_env) reads `NEAR_ACCOUNT_ID` and
`NEAR_PRIVATE_KEY`; [`from_env_vars`](crate::signer::from_env_vars) reads
variables you name, for a process that signs as several accounts. A missing
variable is `Error::Config`, and a bad value is a parse error.

```rust,no_run
use near_kit::*;

# fn example() -> Result<(), Error> {
let near = Near::testnet().build();
let alice = near.with_signer(signer::from_env()?);
let relayer = near.with_signer(signer::from_env_vars("RELAYER_ACCOUNT_ID", "RELAYER_PRIVATE_KEY")?);
# Ok(())
# }
```

[`SigningNear::from_env`](crate::SigningNear::from_env) reads the same two
variables plus the network settings, and builds the client too (see the
[clients chapter](crate::guide::clients)).

### From a near-cli credentials file

With the `file-signer` feature,
[`from_credentials(network, account)`](crate::signer::from_credentials)
reads `~/.near-credentials/<network>/<account>.json`, the file near-cli and
near-cli-rs write. [`from_credentials_file`](crate::signer::from_credentials_file)
reads a file at any path:

```rust
use near_kit::signer::{self, SecretKey, Signer};
use near_kit::*;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let key = SecretKey::generate_ed25519();
let path = std::env::temp_dir().join(format!("guide-keys-{}.json", std::process::id()));
std::fs::write(
    &path,
    serde_json::json!({
        "account_id": "alice.testnet",
        "public_key": key.public_key(),
        "private_key": key,
    })
    .to_string(),
)?;

let alice = signer::from_credentials_file(&path, account!("alice.testnet"))?;
assert_eq!(alice.public_key(), key.public_key());
# std::fs::remove_file(&path)?;
# Ok(())
# }
```

The file needs `private_key` (or `secret_key`). When it also has a
`public_key`, near-kit checks that the private key derives it, and a
mismatch is `Error::Config` naming the file. The account comes from the
argument, not the file.

### From the system keyring

With the `keyring` feature,
[`from_keyring(network, account, &public_key)`](crate::signer::from_keyring)
reads a key that near-cli-rs stored in the macOS Keychain, Windows
Credential Manager or a Linux Secret Service. Keyring entries are named
after the account and the public key, so the public key is an argument.
For an ML-DSA-65 key, near-kit looks for an entry named after the key's
`ml-dsa-65-hash:` handle first, then after the full key:

```rust,no_run
use near_kit::signer::{self, PublicKey};
use near_kit::*;

# fn example() -> Result<(), Error> {
let public_key: PublicKey = "ed25519:6E8sCci9badyRkXb3JoRpBj5p8C6Tw41ELDZoiihKEtp".parse()?;
let alice = Near::testnet()
    .build()
    .with_signer(signer::from_keyring("testnet", account!("alice.testnet"), &public_key)?);
# Ok(())
# }
```

A missing keyring service, a missing entry, an entry in an unknown format
or an entry holding a different key is `Error::KeyStore`.

### From a seed phrase

With the `mnemonic` feature,
[`SecretKey::from_seed_phrase`](crate::signer::SecretKey::from_seed_phrase)
derives a key from a BIP-39 phrase, and
[`generate_with_seed_phrase`](crate::signer::SecretKey::generate_with_seed_phrase)
makes a new phrase and its key:

```rust
use near_kit::signer::{InMemorySigner, KeyType, SecretKey};
use near_kit::*;

# fn main() -> Result<(), Box<dyn std::error::Error>> {
let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

// The default path m/44'/397'/0' and no passphrase: what near-cli and wallets derive.
let key = SecretKey::from_seed_phrase(KeyType::Ed25519, phrase, None, None)?;
let alice = InMemorySigner::new(account!("alice.testnet"), key);

// Another key from the same phrase.
let second = SecretKey::from_seed_phrase(KeyType::Ed25519, phrase, Some("m/44'/397'/1'"), None)?;

// A new phrase to show the user once, and its key.
let (phrase, key) = SecretKey::generate_with_seed_phrase(KeyType::Ed25519, None)?;
assert_eq!(phrase.split_whitespace().count(), 12);
# Ok(())
# }
```

Ed25519 derivation is SLIP-10, as near-cli and wallets use. ML-DSA-65
derivation follows NEP-649; the ML-DSA-65 key from a phrase is unrelated to
the ed25519 key from the same phrase. New ML-DSA-65 phrases have 24 words
by default, and fewer than 18 is rejected; recovery accepts any valid
length. Secp256k1 has no seed-phrase derivation and fails with
`SignerError::KeyDerivationFailed`.

## Implicit accounts

An implicit account's ID is the hex of its ed25519 public key, so a key is
enough to have an account: it exists on chain once someone sends it NEAR.
[`InMemorySigner::implicit`](crate::signer::InMemorySigner::implicit)
builds the signer:

```rust,no_run
use near_kit::protocol::Action;
use near_kit::signer::{InMemorySigner, SecretKey, Signer};
use near_kit::*;

# async fn example(alice: &SigningNear) -> Result<(), Box<dyn std::error::Error>> {
let key = SecretKey::generate_ed25519();
let fresh = InMemorySigner::implicit(key)?;

// Funding the ID creates the account, with the key as its full-access key.
alice
    .transaction(fresh.account_id())
    .add_action(Action::transfer(NearToken::from_near(1)))
    .await?.result()?;

let fresh = alice.near().with_signer(fresh);
# Ok(())
# }
```

Only ed25519 keys have implicit accounts; others fail with
`SignerError::ImplicitAccountRequiresEd25519`.

## Several keys for one account

[`RotatingSigner`](crate::signer::RotatingSigner) holds several keys of one
account and hands out the next one for each transaction, so concurrent
sends don't contend for one nonce. Build it from keys
(`RotatingSigner::new(account, keys)`) or from loaded signers
(`RotatingSigner::from_signers(vec![..])`, which requires them all to be for
the same account). The [nonces chapter](crate::guide::nonces) covers when
it helps and how to add the keys.

## Keys near-kit can't see

For a hardware wallet, an HSM, a cloud KMS or a remote signing service,
implement [`SigningBackend`](crate::signer::SigningBackend): one async
method that signs a message. Wrap it with
[`SigningKey::from_backend`](crate::signer::SigningKey::from_backend) and
the key's public key, and return that from a
[`Signer`](crate::signer::Signer). A `SigningNear` over that signer sends,
signs and signs delegate actions as with an in-memory key:

```rust,no_run
use near_kit::signer::{PublicKey, Signature, Signer, SignerError, SigningBackend, SigningKey};
use near_kit::*;

/// Signs through a remote service. `request_signature` stands in for its client.
struct RemoteKey {
    key_id: String,
}

# async fn request_signature(_key_id: &str, _message: &[u8]) -> Result<[u8; 64], String> { Ok([0; 64]) }
impl SigningBackend for RemoteKey {
    async fn sign(&self, message: &[u8]) -> Result<Signature, SignerError> {
        let bytes = request_signature(&self.key_id, message)
            .await
            .map_err(SignerError::SigningFailed)?;
        Ok(Signature::ed25519_from_bytes(bytes))
    }
}

struct RemoteSigner {
    account_id: AccountId,
    key: SigningKey,
}

impl Signer for RemoteSigner {
    fn account_id(&self) -> &AccountId {
        &self.account_id
    }

    fn key(&self) -> SigningKey {
        self.key.clone()
    }

    fn public_key(&self) -> PublicKey {
        self.key.public_key().clone()
    }
}

# fn example(public_key: PublicKey) {
let signer = RemoteSigner {
    account_id: account!("treasury.near"),
    key: SigningKey::from_backend(public_key, RemoteKey { key_id: "treasury-1".into() }),
};
let treasury = Near::mainnet().build().with_signer(signer);
# }
```

The messages near-kit passes to a backend are 32-byte hashes: of a
transaction, a delegate action or a NEP-413 payload. Sign them as they
are; a backend that hashes them again produces signatures the chain
rejects. near-kit calls `key` once per transaction and uses the returned
key's public key for the nonce, the transaction and the signature, so a
signer that rotates keys can return a different `SigningKey` from each
call. `public_key` reports the key the next `key` call would return, and
must have no side effects: no I/O, no prompt, no rotation.

## See also

- [`SecretKey`](crate::signer::SecretKey),
  [`PublicKey`](crate::signer::PublicKey),
  [`PublicKeyHandle`](crate::signer::PublicKeyHandle),
  [`Signature`](crate::signer::Signature),
  [`ParseKeyError`](crate::signer::ParseKeyError)
- [`Signer`](crate::signer::Signer),
  [`InMemorySigner`](crate::signer::InMemorySigner),
  [`RotatingSigner`](crate::signer::RotatingSigner),
  [`SigningKey`](crate::signer::SigningKey),
  [`SigningBackend`](crate::signer::SigningBackend)
- The loaders in [`signer`](crate::signer)
- The [offline signing chapter](crate::guide::offline_signing) for signing
  without a client, and NEP-413 messages
