//! SLIP-10 hardened hierarchical deterministic key derivation.
//!
//! Implements the Ed25519 branch of SLIP-0010
//! (<https://github.com/satoshilabs/slips/blob/master/slip-0010.md>) and the
//! ML-DSA-65 branch of the analogous post-quantum construction from
//! satoshilabs/slips#1968 (adopted by near/devex#58), specified for NEAR by
//! NEP-649 (<https://github.com/near/NEPs/pull/649>). Both branches share the
//! identical SLIP-10 machinery — the same hardened-only child step over
//! HMAC-SHA512 — and differ *only* in the master HMAC salt: `"ed25519 seed"`
//! for Ed25519, `"ML-DSA-65 seed"` for ML-DSA-65. As a result the two keys
//! derived from the same BIP-39 seed are unrelated.
//!
//! Only hardened derivation is supported — non-hardened components are rejected
//! by [`parse_hd_path`].

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha512;

type HmacSha512 = Hmac<Sha512>;

/// BIP-32 hardened derivation offset (2^31).
const HARDENED: u32 = 0x8000_0000;

/// Master HMAC salt for the Ed25519 SLIP-0010 branch.
const ED25519_SEED_SALT: &[u8] = b"ed25519 seed";

/// Master HMAC salt for the ML-DSA-65 branch (satoshilabs/slips#1968).
const ML_DSA65_SEED_SALT: &[u8] = b"ML-DSA-65 seed";

/// Error parsing a BIP-32 path string for SLIP-10 derivation.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HdPathError {
    /// A path segment was empty (e.g. `"m//0'"` or a trailing `/`).
    EmptySegment,
    /// A segment was not a decimal integer optionally suffixed with `'` or `H`.
    InvalidIndex(String),
    /// The raw index was ≥ 2^31 (the hardened bit position).
    IndexOutOfRange(String),
    /// SLIP-10 (both the Ed25519 and ML-DSA-65 branches) requires every
    /// component to be hardened (`'` or `H` suffix).
    NotHardened(String),
}

impl std::fmt::Display for HdPathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySegment => write!(f, "empty path segment"),
            Self::InvalidIndex(s) => write!(f, "invalid path index {s:?}"),
            Self::IndexOutOfRange(s) => write!(f, "path index out of range {s:?}"),
            Self::NotHardened(s) => {
                write!(f, "SLIP-10 requires hardened derivation, got {s:?}")
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

/// Derive the full 64-byte SLIP-10 node `I = I_L || I_R` (the secret scalar
/// followed by the chain code) from `seed` along `path`, using `salt` as the
/// master HMAC key.
///
/// This is the shared engine behind [`derive_ed25519_slip10`] and
/// [`derive_ml_dsa65_slip10`]; the two branches differ only in `salt`. `path`
/// must be a slice of already-hardened indexes (each with the high bit set).
/// Use [`parse_hd_path`] to produce one from a string.
fn derive_slip10_node(salt: &[u8], seed: &[u8], path: &[u32]) -> [u8; 64] {
    // Master: I = HMAC-SHA512(key=salt, data=seed)
    let mut mac = HmacSha512::new_from_slice(salt).expect("HMAC accepts any key length");
    mac.update(seed);
    let mut i = mac.finalize().into_bytes();

    for &index in path {
        debug_assert!(
            index & HARDENED != 0,
            "SLIP-10 requires hardened indexes; got {index:#x}"
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

    let mut node = [0u8; 64];
    node.copy_from_slice(&i);
    node
}

/// Derive a 32-byte Ed25519 secret scalar from `seed` along `path` using
/// SLIP-10 for the Ed25519 curve (master salt `"ed25519 seed"`).
///
/// `path` must be a slice of already-hardened indexes (each with the high
/// bit set). Use [`parse_hd_path`] to produce one from a string.
pub(crate) fn derive_ed25519_slip10(seed: &[u8], path: &[u32]) -> [u8; 32] {
    let node = derive_slip10_node(ED25519_SEED_SALT, seed, path);
    let mut key = [0u8; 32];
    key.copy_from_slice(&node[..32]);
    key
}

/// Derive a 32-byte ML-DSA-65 node secret (the FIPS-204 seed ξ) from `seed`
/// along `path` using the SLIP-10 construction from satoshilabs/slips#1968,
/// specified for NEAR by NEP-649 (<https://github.com/near/NEPs/pull/649>)
/// (master salt `"ML-DSA-65 seed"`). Feed the result to ML-DSA-65 KeyGen.
///
/// `path` must be a slice of already-hardened indexes (each with the high
/// bit set). Use [`parse_hd_path`] to produce one from a string.
pub(crate) fn derive_ml_dsa65_slip10(seed: &[u8], path: &[u32]) -> [u8; 32] {
    let node = derive_slip10_node(ML_DSA65_SEED_SALT, seed, path);
    let mut key = [0u8; 32];
    key.copy_from_slice(&node[..32]);
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
    // ML-DSA-65 test vectors (satoshilabs/slips#1968, adopted by near/devex#58
    // and specified for NEAR by NEP-649, https://github.com/near/NEPs/pull/649)
    //
    // Same SLIP-10 machinery as the ed25519 vectors above, but with the master
    // salt "ML-DSA-65 seed"; the 32-byte node secret I_L is the FIPS-204 seed ξ
    // fed to ML-DSA-65 KeyGen. Each row is [path, chain_code, sha256(pubkey)].
    // We assert the chain code verbatim and the SHA-256 of the 1952-byte
    // ML-DSA-65 public key derived from I_L (the raw keys are too large to
    // inline). These are the verbatim slips#1968 vectors, which are themselves
    // validated against the NIST ACVP ML-DSA-keyGen-FIPS204 KATs.
    // ------------------------------------------------------------------------

    fn assert_ml_dsa65_vectors(seed_hex: &str, rows: &[(&str, &str, &str)]) {
        use sha2::{Digest, Sha256};

        let seed = unhex(seed_hex);
        for (path, chain_code, pk_sha256) in rows {
            let node = derive_slip10_node(ML_DSA65_SEED_SALT, &seed, &parse_hd_path(path).unwrap());
            assert_eq!(
                hex::encode(&node[32..]),
                *chain_code,
                "chain code for {path}"
            );

            // Derive the ML-DSA-65 public key from I_L via the real key path.
            let il: [u8; 32] = node[..32].try_into().unwrap();
            let public = crate::types::key::SecretKey::ml_dsa65_from_seed(il).public_key();
            let raw = public.as_ml_dsa65_bytes().expect("full ML-DSA-65 key");
            assert_eq!(
                hex::encode(Sha256::digest(raw.as_slice())),
                *pk_sha256,
                "pubkey digest for {path}"
            );
        }
    }

    #[test]
    fn slip1968_vec1_ml_dsa65() {
        assert_ml_dsa65_vectors(
            "000102030405060708090a0b0c0d0e0f",
            &[
                (
                    "m",
                    "7e74b6275f92cc4fb2cbdac0c63cb5e7ac2bce1ded2b7dbc7bf2232f772578d5",
                    "f41b8366cd9b720dbab9dfcefde673e4c19798192d7543f30f277e57e77ba457",
                ),
                (
                    "m/0'",
                    "d8b27c87ec212d6501629199262a9d0d66ec26deab313c26a474bc4ddd5dce7c",
                    "1d9d7fce0a1560acef9b117f3e3022cb0bdd46f30a3134db33165baf66b53e7e",
                ),
                (
                    "m/0'/1'",
                    "60219beef857bd0bc7870424c4f60464decb097a18ae37b035df22ebaa7515b9",
                    "0fdc416cbe544a493b59ae8112a907d5acf09ed4583eb8e12d6c769bcf394943",
                ),
                (
                    "m/0'/1'/2'",
                    "dc7b0e1379b3f1acd02d1e25f13d8bb16830ca68c73b00d900b9030a41d6a658",
                    "6db4146987c59099709622700d66bcb07b8d49e1007ea6570e15fc3f6dac1c53",
                ),
                (
                    "m/0'/1'/2'/2'",
                    "565cb34027c3b54773f7f48e329bc86db7ffc6f618b112af6d59a3f82501e17a",
                    "d487dcf16e74ddd731fafe780d0e5e8b8d338d4f7a3b7e209f8b4c519419764d",
                ),
                (
                    "m/0'/1'/2'/2'/1000000000'",
                    "dc65d6cf4fa993c2f04fae2b41d70d8c5ba4c9d2042ea5720bf42a2315a6b7db",
                    "9591ff122a7eff9cd64100f2123e678db9c257815ec5570b0b6acd8847e62f24",
                ),
            ],
        );
    }

    #[test]
    fn slip1968_vec2_ml_dsa65() {
        assert_ml_dsa65_vectors(
            "fffcf9f6f3f0edeae7e4e1dedbd8d5d2cfccc9c6c3c0bdbab7b4b1aeaba8a5a2\
             9f9c999693908d8a8784817e7b7875726f6c696663605d5a5754514e4b484542",
            &[
                (
                    "m",
                    "0e696f43f0e71c1c9febf61f43f10903385b78cee5871915472027b25ba755cc",
                    "36233a01384e7081becf4da903d10fe7d357da9061cbed1983819fe0a0ced509",
                ),
                (
                    "m/0'",
                    "d977be3c8525364e155764ef76b985126d8f6be41b55e6e50a9915ae29f27a56",
                    "577dc77b0987d6cc587a88605590b183f3a7580577dab08cac1ede0a9636b882",
                ),
                (
                    "m/0'/2147483647'",
                    "45406bac7fc36390d89ac938bff6ca1ebf9fc6da2057d1580b26a8f2e25a0d2f",
                    "a0155112060496906db18bd46cdd7e18dd35e6e14621083300a0027fd6a421a2",
                ),
                (
                    "m/0'/2147483647'/1'",
                    "10ffddec3c4acf719afe528b1ced03e0f7d8555999c11b69376bd5ff4e04dbea",
                    "41357d47d1269fe94f9fc66396d7dc768e5bcaae700a0b877471a06ef2e12d27",
                ),
                (
                    "m/0'/2147483647'/1'/2147483646'",
                    "3580d842e9d8d6a072b77d95e08b0a87648edfaec9bc25cee6c9127d1aa4a679",
                    "e947963f404e03be513bdccf23c5a3cb9bd4455f306276fcd9acd4b0d3e16e75",
                ),
                (
                    "m/0'/2147483647'/1'/2147483646'/2'",
                    "55a202d3f7803dd79e31c454e65eb7da49dee824b467bfed5204df980e71571d",
                    "4b6c9f1dd1a811fe704c1355e32c43b63007317868ed6c3fce385f11c1b5da39",
                ),
            ],
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
