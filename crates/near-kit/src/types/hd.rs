//! SLIP-10 Ed25519 hierarchical deterministic key derivation.
//!
//! Implements the Ed25519 branch of SLIP-0010
//! (<https://github.com/satoshilabs/slips/blob/master/slip-0010.md>).
//!
//! Ed25519 SLIP-10 only supports hardened derivation — non-hardened
//! components are rejected by [`parse_hd_path`].

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha512;

type HmacSha512 = Hmac<Sha512>;

/// BIP-32 hardened derivation offset (2^31).
const HARDENED: u32 = 0x8000_0000;

/// Error parsing a BIP-32 path string for Ed25519 SLIP-10 derivation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HdPathError {
    /// A path segment was empty (e.g. `"m//0'"` or a trailing `/`).
    EmptySegment,
    /// A segment was not a decimal integer optionally suffixed with `'` or `H`.
    InvalidIndex(String),
    /// The raw index was ≥ 2^31 (the hardened bit position).
    IndexOutOfRange(String),
    /// Ed25519 SLIP-10 requires every component to be hardened (`'` or `H` suffix).
    NotHardened(String),
}

impl std::fmt::Display for HdPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySegment => write!(f, "empty path segment"),
            Self::InvalidIndex(s) => write!(f, "invalid path index {s:?}"),
            Self::IndexOutOfRange(s) => write!(f, "path index out of range {s:?}"),
            Self::NotHardened(s) => {
                write!(f, "ed25519 requires hardened derivation, got {s:?}")
            }
        }
    }
}

/// Parse a BIP-32 path like `m/44'/397'/0'` into a list of hardened indexes
/// (high bit set). Accepts `'` or `H` as the hardened marker.
///
/// Root-path forms `""`, `"m"`, and `"m/"` all derive the master key only.
/// A single leading `/` (with or without the `m`) and a single trailing `/`
/// are tolerated; repeated or interior empty segments are rejected.
pub(crate) fn parse_hd_path(path: &str) -> Result<Vec<u32>, HdPathError> {
    // Normalize: strip optional leading `m`, then a single leading `/`,
    // then a single trailing `/`. Anything left is the `/`-separated body.
    let body = path.strip_prefix('m').unwrap_or(path);
    let body = body.strip_prefix('/').unwrap_or(body);
    let body = body.strip_suffix('/').unwrap_or(body);
    if body.is_empty() {
        return Ok(Vec::new());
    }

    body.split('/')
        .map(|seg| {
            if seg.is_empty() {
                return Err(HdPathError::EmptySegment);
            }
            let (num_str, hardened) = match seg.as_bytes().last() {
                Some(b'\'') | Some(b'H') => (&seg[..seg.len() - 1], true),
                _ => (seg, false),
            };
            if num_str.is_empty() || !num_str.bytes().all(|b| b.is_ascii_digit()) {
                return Err(HdPathError::InvalidIndex(seg.to_string()));
            }
            let idx: u32 = num_str
                .parse()
                .map_err(|_| HdPathError::IndexOutOfRange(seg.to_string()))?;
            if idx >= HARDENED {
                return Err(HdPathError::IndexOutOfRange(seg.to_string()));
            }
            if !hardened {
                return Err(HdPathError::NotHardened(seg.to_string()));
            }
            Ok(idx | HARDENED)
        })
        .collect()
}

/// Derive a 32-byte Ed25519 secret scalar from `seed` along `path` using
/// SLIP-10 for the Ed25519 curve.
///
/// `path` must be a slice of already-hardened indexes (each with the high
/// bit set). Use [`parse_hd_path`] to produce one from a string.
pub(crate) fn derive_ed25519_slip10(seed: &[u8], path: &[u32]) -> [u8; 32] {
    // Master: I = HMAC-SHA512(key="ed25519 seed", data=seed)
    let mut mac = HmacSha512::new_from_slice(b"ed25519 seed").expect("HMAC accepts any key length");
    mac.update(seed);
    let mut i = mac.finalize().into_bytes();

    for &index in path {
        debug_assert!(
            index & HARDENED != 0,
            "ed25519 SLIP-10 requires hardened indexes; got {index:#x}"
        );
        // Hardened child: Data = 0x00 || I_L(parent) || ser32(index)
        let (il, ir) = i.split_at(32);

        let mut data = [0u8; 1 + 32 + 4];
        data[0] = 0x00;
        data[1..33].copy_from_slice(il);
        data[33..].copy_from_slice(&index.to_be_bytes());

        let mut mac = HmacSha512::new_from_slice(ir).expect("HMAC accepts any key length");
        mac.update(&data);
        i = mac.finalize().into_bytes();
    }

    let mut key = [0u8; 32];
    key.copy_from_slice(&i[..32]);
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        hex::decode(s).expect("valid hex")
    }

    // ------------------------------------------------------------------------
    // SLIP-10 official Ed25519 test vectors
    // https://github.com/satoshilabs/slips/blob/master/slip-0010.md
    // ------------------------------------------------------------------------

    #[test]
    fn slip10_vec1_ed25519() {
        let seed = unhex("000102030405060708090a0b0c0d0e0f");

        // Master (chain m)
        assert_eq!(
            hex::encode(derive_ed25519_slip10(&seed, &[])),
            "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7"
        );
        // m/0'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'").unwrap()
            )),
            "68e0fe46dfb67e368c75379acec591dad19df3cde26e63b93a8e704f1dade7a3"
        );
        // m/0'/1'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'/1'").unwrap()
            )),
            "b1d0bad404bf35da785a64ca1ac54b2617211d2777696fbffaf208f746ae84f2"
        );
        // m/0'/1'/2'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'/1'/2'").unwrap()
            )),
            "92a5b23c0b8a99e37d07df3fb9966917f5d06e02ddbd909c7e184371463e9fc9"
        );
        // m/0'/1'/2'/2'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'/1'/2'/2'").unwrap()
            )),
            "30d1dc7e5fc04c31219ab25a27ae00b50f6fd66622f6e9c913253d6511d1e662"
        );
        // m/0'/1'/2'/2'/1000000000'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'/1'/2'/2'/1000000000'").unwrap()
            )),
            "8f94d394a8e8fd6b1bc2f3f49f5c47e385281d5c17e65324b0f62483e37e8793"
        );
    }

    #[test]
    fn slip10_vec2_ed25519() {
        let seed = unhex(
            "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a2\
             9f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
        );

        // Master
        assert_eq!(
            hex::encode(derive_ed25519_slip10(&seed, &[])),
            "171cb88b1b3c1db25add599712e36245d75bc65a1a5c9e18d76f9f2b1eab4012"
        );
        // m/0'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'").unwrap()
            )),
            "1559eb2bbec5790b0c65d8693e4d0875b1747f4970ae8b650486ed7470845635"
        );
        // m/0'/2147483647'/1'/2147483646'/2'
        assert_eq!(
            hex::encode(derive_ed25519_slip10(
                &seed,
                &parse_hd_path("m/0'/2147483647'/1'/2147483646'/2'").unwrap()
            )),
            "551d333177df541ad876a60ea71f00447931c0a9da16f227c11ea080d7391b8d"
        );
    }

    // ------------------------------------------------------------------------
    // Path parser
    // ------------------------------------------------------------------------

    #[test]
    fn parse_accepts_common_forms() {
        // Root-path forms all derive the master key
        let empty = Vec::<u32>::new();
        assert_eq!(parse_hd_path("").unwrap(), empty);
        assert_eq!(parse_hd_path("m").unwrap(), empty);
        assert_eq!(parse_hd_path("m/").unwrap(), empty);

        assert_eq!(
            parse_hd_path("m/44'/397'/0'").unwrap(),
            vec![44 | HARDENED, 397 | HARDENED, HARDENED]
        );
        // H and ' are interchangeable
        assert_eq!(
            parse_hd_path("m/44H/397H/0H").unwrap(),
            parse_hd_path("m/44'/397'/0'").unwrap()
        );
        // leading "m/" is optional
        assert_eq!(
            parse_hd_path("44'/397'/0'").unwrap(),
            parse_hd_path("m/44'/397'/0'").unwrap()
        );
        // A single trailing slash is tolerated (matches slipped10's behavior)
        assert_eq!(
            parse_hd_path("m/44'/397'/0'/").unwrap(),
            parse_hd_path("m/44'/397'/0'").unwrap()
        );
    }

    #[test]
    fn parse_rejects_bad_input() {
        // Interior empty segment
        assert!(matches!(
            parse_hd_path("m/44'//0'"),
            Err(HdPathError::EmptySegment)
        ));
        // Double trailing slash: one is stripped, the other is an empty segment
        assert!(matches!(
            parse_hd_path("m/44'//"),
            Err(HdPathError::EmptySegment)
        ));
        // Non-hardened is rejected for ed25519
        assert!(matches!(
            parse_hd_path("m/44'/397'/0"),
            Err(HdPathError::NotHardened(_))
        ));
        // Negative / non-numeric
        assert!(matches!(
            parse_hd_path("m/-1'"),
            Err(HdPathError::InvalidIndex(_))
        ));
        assert!(matches!(
            parse_hd_path("m/abc'"),
            Err(HdPathError::InvalidIndex(_))
        ));
        // Overflow past 2^31
        assert!(matches!(
            parse_hd_path("m/2147483648'"),
            Err(HdPathError::IndexOutOfRange(_))
        ));
    }
}
