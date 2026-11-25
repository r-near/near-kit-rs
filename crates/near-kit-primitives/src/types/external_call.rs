//! External contract calls (NEP-667).

use std::io::{self, Read, Write};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use borsh::{BorshDeserialize, BorshSerialize};

use super::{AccountId, CryptoHash, RawStateInit};
use crate::error::DecodeTransactionError;

/// The contract method an external call runs. A `FunctionCall` action to it
/// fails with `ActionErrorKind::ReservedMethodName`.
pub const EXTERNAL_CALL_METHOD: &str = "__external_call";

/// First byte of an external call's encoding, which no signed transaction
/// starts with.
const ESCAPE_BYTE: u8 = 0xFF;

/// The version of the encoding, the byte after [`ESCAPE_BYTE`].
const VERSION: u8 = 0;

/// An external contract call ([NEP-667]): a transaction with no signer, key,
/// nonce or signature, which `contract_id` authorizes in its
/// `__external_call` method and pays for from its own balance.
///
/// Nobody signs it, so any node it passes through can change it. The
/// contract checks the payload; carry a signature over the payload's exact
/// bytes inside it, covering the chain id, the contract, a nonce and an
/// expiry.
///
/// A call can be included more than once. Each inclusion is a copy, whose
/// outcome id is its [`unique_id`](Self::unique_id) at the height of the block
/// that includes it.
///
/// ```rust
/// use near_kit_primitives::account;
/// use near_kit_primitives::protocol::ExternalCall;
///
/// let call = ExternalCall::new(account!("wallet.near"), vec![1, 2, 3]);
/// let bytes = call.to_bytes();
/// assert_eq!(&bytes[..2], &[0xFF, 0x00]);
/// assert_eq!(ExternalCall::from_bytes(&bytes)?, call);
/// assert_ne!(call.unique_id(100), call.unique_id(101));
/// # Ok::<(), near_kit_primitives::protocol::DecodeTransactionError>(())
/// ```
///
/// [NEP-667]: https://github.com/near/NEPs/pull/667
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalCall {
    contract_id: AccountId,
    payload: Vec<u8>,
    state_init: Option<RawStateInit>,
}

impl ExternalCall {
    /// A call to `contract_id`'s `__external_call` with `payload` as its
    /// input, and no state init.
    pub fn new(contract_id: impl Into<AccountId>, payload: impl Into<Vec<u8>>) -> Self {
        Self {
            contract_id: contract_id.into(),
            payload: payload.into(),
            state_init: None,
        }
    }

    /// The same call carrying a universal account's state init, installed
    /// before the run when `contract_id` is not initialized yet. It must
    /// derive to `contract_id`. A call to an initialized account is included
    /// without it; the [`unique_id`](Self::unique_id) is the same either way.
    pub fn with_state_init(mut self, state_init: impl Into<RawStateInit>) -> Self {
        self.state_init = Some(state_init.into());
        self
    }

    /// The contract that authorizes, pays for and receives the call.
    pub fn contract_id(&self) -> &AccountId {
        &self.contract_id
    }

    /// The call's input, which the contract decodes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// The state init the call carries, if any.
    pub fn state_init(&self) -> Option<&RawStateInit> {
        self.state_init.as_ref()
    }

    /// The hash every copy's [`unique_id`](Self::unique_id) comes from: the
    /// SHA-256 of the call's encoding without its state init, so a copy
    /// that carries it and one that does not share it.
    ///
    /// `tx` does not take this hash; look a copy up by its unique id.
    pub fn call_hash(&self) -> CryptoHash {
        let mut bytes = Vec::new();
        self.write(&mut bytes, None)
            .expect("writing to a Vec cannot fail");
        CryptoHash::hash(&bytes)
    }

    /// The outcome id of the copy included in the block at `height`:
    /// `sha256(call_hash || height as u64 little endian)`. Pass it to `tx`
    /// as the transaction hash, with the contract as the sender.
    pub fn unique_id(&self, height: u64) -> CryptoHash {
        let mut bytes = [0u8; 40];
        bytes[..32].copy_from_slice(self.call_hash().as_bytes());
        bytes[32..].copy_from_slice(&height.to_le_bytes());
        CryptoHash::hash(&bytes)
    }

    /// The call's encoding, which `send_tx` takes base64-encoded in
    /// `external_call_base64`: `0xFF`, the version `0`, then the borsh of
    /// `contract_id`, `payload` and `state_init`.
    pub fn to_bytes(&self) -> Vec<u8> {
        borsh::to_vec(self).expect("writing to a Vec cannot fail")
    }

    /// [`to_bytes`](Self::to_bytes) in base64.
    pub fn to_base64(&self) -> String {
        BASE64_STANDARD.encode(self.to_bytes())
    }

    /// Decode a call from its encoding (round-trips
    /// [`to_bytes`](Self::to_bytes)). Rejects another escape byte, an unknown
    /// version and trailing bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, DecodeTransactionError> {
        borsh::from_slice(bytes).map_err(DecodeTransactionError::Borsh)
    }

    /// Decode a call from base64 (round-trips [`to_base64`](Self::to_base64)).
    pub fn from_base64(s: &str) -> Result<Self, DecodeTransactionError> {
        let bytes = BASE64_STANDARD
            .decode(s)
            .map_err(|e| DecodeTransactionError::Base64(e.to_string()))?;
        Self::from_bytes(&bytes)
    }
}

impl ExternalCall {
    /// Write the encoding with `state_init` in place of the call's own.
    fn write<W: Write>(&self, writer: &mut W, state_init: Option<&RawStateInit>) -> io::Result<()> {
        ESCAPE_BYTE.serialize(writer)?;
        VERSION.serialize(writer)?;
        self.contract_id.serialize(writer)?;
        self.payload.serialize(writer)?;
        state_init.serialize(writer)
    }
}

impl BorshSerialize for ExternalCall {
    fn serialize<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        self.write(writer, self.state_init.as_ref())
    }
}

impl BorshDeserialize for ExternalCall {
    fn deserialize_reader<R: Read>(reader: &mut R) -> io::Result<Self> {
        let escape_byte = u8::deserialize_reader(reader)?;
        if escape_byte != ESCAPE_BYTE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("not an external call: first byte {escape_byte:#04x}, expected 0xff"),
            ));
        }
        let version = u8::deserialize_reader(reader)?;
        if version != VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported external call version {version}"),
            ));
        }
        Ok(Self {
            contract_id: AccountId::deserialize_reader(reader)?,
            payload: Vec::deserialize_reader(reader)?,
            state_init: Option::deserialize_reader(reader)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn encoding_is_escape_byte_version_then_borsh_fields() {
        let call = ExternalCall::new(account!("ab"), vec![7]);
        // 0xFF, version 0, "ab" (u32 len 2), payload (u32 len 1, 7), None.
        assert_eq!(hex(&call.to_bytes()), "ff00020000006162010000000700");
        let with = call.clone().with_state_init(RawStateInit(vec![4, 5]));
        assert_eq!(
            hex(&with.to_bytes()),
            "ff00020000006162010000000701020000000405"
        );
    }

    #[test]
    fn accessors_return_the_parts_the_call_was_built_from() {
        let call = ExternalCall::new(account!("wallet.near"), vec![1, 2, 3]);
        assert_eq!(call.contract_id(), "wallet.near");
        assert_eq!(call.payload(), [1, 2, 3]);
        assert_eq!(call.state_init(), None);
        let with = call.with_state_init(RawStateInit(vec![4, 5, 6]));
        assert_eq!(with.payload(), [1, 2, 3]);
        assert_eq!(with.state_init(), Some(&RawStateInit(vec![4, 5, 6])));
    }

    #[test]
    fn decode_round_trips_and_rejects_bad_prefixes() {
        let call = ExternalCall::new(account!("wallet.near"), vec![1, 2, 3])
            .with_state_init(RawStateInit(vec![4, 5, 6]));
        assert_eq!(ExternalCall::from_bytes(&call.to_bytes()).unwrap(), call);
        assert_eq!(ExternalCall::from_base64(&call.to_base64()).unwrap(), call);

        let mut wrong_escape = call.to_bytes();
        wrong_escape[0] = 0xFE;
        let err = ExternalCall::from_bytes(&wrong_escape).unwrap_err();
        assert!(err.to_string().contains("0xfe"), "{err}");

        let mut wrong_version = call.to_bytes();
        wrong_version[1] = 1;
        let err = ExternalCall::from_bytes(&wrong_version).unwrap_err();
        assert!(err.to_string().contains("version 1"), "{err}");

        let mut trailing = call.to_bytes();
        trailing.push(0);
        assert!(ExternalCall::from_bytes(&trailing).is_err());
        assert!(ExternalCall::from_bytes(&[]).is_err());
        assert!(ExternalCall::from_bytes(&[0xFF]).is_err());
        assert!(matches!(
            ExternalCall::from_base64("not base64!"),
            Err(DecodeTransactionError::Base64(_))
        ));
    }

    #[test]
    fn call_hash_ignores_the_state_init() {
        let call = ExternalCall::new(account!("wallet.near"), vec![1, 2, 3]);
        let with = call.clone().with_state_init(RawStateInit(vec![4, 5, 6]));
        assert_eq!(call.call_hash(), with.call_hash());
        assert_eq!(call.unique_id(1), with.unique_id(1));
        assert_eq!(call.call_hash(), CryptoHash::hash(&call.to_bytes()));
        assert_ne!(CryptoHash::hash(&with.to_bytes()), with.call_hash());
    }

    #[test]
    fn unique_id_is_sha256_of_call_hash_and_little_endian_height() {
        let call = ExternalCall::new(account!("wallet.near"), vec![1, 2, 3]);
        for height in [0, 1, 0x0102_0304_0506_0708, u64::MAX] {
            let mut bytes = call.call_hash().as_bytes().to_vec();
            bytes.extend_from_slice(&height.to_le_bytes());
            assert_eq!(call.unique_id(height), CryptoHash::hash(&bytes));
        }
        assert_ne!(call.unique_id(7), call.unique_id(8));
        assert_ne!(
            call.unique_id(7),
            ExternalCall::new(account!("wallet.near"), vec![1, 2]).unique_id(7)
        );
        assert_ne!(
            call.unique_id(7),
            ExternalCall::new(account!("other.near"), vec![1, 2, 3]).unique_id(7)
        );
    }
}
