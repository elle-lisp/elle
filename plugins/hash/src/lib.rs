//! Elle hash plugin — universal hashing with MD5, SHA-1, SHA-2, SHA-3,
//! BLAKE2, BLAKE3, CRC32, and xxHash.

use blake2::{Blake2b512, Blake2s256};
use crc32fast::Hasher as Crc32;
use digest::Digest;
use md5::Md5;
use sha1::Sha1;
use sha2::{Sha224, Sha256, Sha384, Sha512, Sha512_224, Sha512_256};
use sha3::{Sha3_224, Sha3_256, Sha3_384, Sha3_512};
use std::cell::RefCell;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------
elle::elle_plugin_init!(PRIMITIVES, "hash/");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract byte data from a string, bytes, or @bytes value.
fn extract_bytes(val: &Value, name: &str, pos: &str) -> Result<Vec<u8>, (SignalBits, Value)> {
    if let Some(bytes) = val.with_string(|s| s.as_bytes().to_vec()) {
        Ok(bytes)
    } else if let Some(b) = val.as_bytes() {
        Ok(b.to_vec())
    } else if let Some(blob_ref) = val.as_bytes_mut() {
        Ok(blob_ref.borrow().clone())
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be string, bytes, or @bytes, got {}",
                    name,
                    pos,
                    val.type_name()
                ),
            ),
        ))
    }
}

/// Extract a string from a Value, or return a type error.
fn extract_string(val: &Value, name: &str, pos: &str) -> Result<String, (SignalBits, Value)> {
    val.with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be a string, got {}",
                    name,
                    pos,
                    val.type_name()
                ),
            ),
        )
    })
}

/// Check arity and extract byte data for a unary hash primitive.
fn oneshot_args(args: &[Value], name: &str) -> Result<Vec<u8>, (SignalBits, Value)> {
    if args.len() != 1 {
        return Err((
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 argument, got {}", name, args.len()),
            ),
        ));
    }
    extract_bytes(&args[0], name, "argument")
}

// ---------------------------------------------------------------------------
// One-shot primitives (Digest-based → bytes)
// ---------------------------------------------------------------------------

macro_rules! digest_prim {
    ($fn_name:ident, $hasher:ty, $prim_name:expr) => {
        fn $fn_name(args: &[Value]) -> (SignalBits, Value) {
            match oneshot_args(args, $prim_name) {
                Ok(data) => (SIG_OK, Value::bytes(<$hasher>::digest(&data).to_vec())),
                Err(e) => e,
            }
        }
    };
}

digest_prim!(prim_md5, Md5, "hash/md5");
digest_prim!(prim_sha1, Sha1, "hash/sha1");
digest_prim!(prim_sha224, Sha224, "hash/sha224");
digest_prim!(prim_sha256, Sha256, "hash/sha256");
digest_prim!(prim_sha384, Sha384, "hash/sha384");
digest_prim!(prim_sha512, Sha512, "hash/sha512");
digest_prim!(prim_sha512_224, Sha512_224, "hash/sha512-224");
digest_prim!(prim_sha512_256, Sha512_256, "hash/sha512-256");
digest_prim!(prim_sha3_224, Sha3_224, "hash/sha3-224");
digest_prim!(prim_sha3_256, Sha3_256, "hash/sha3-256");
digest_prim!(prim_sha3_384, Sha3_384, "hash/sha3-384");
digest_prim!(prim_sha3_512, Sha3_512, "hash/sha3-512");
digest_prim!(prim_blake2b512, Blake2b512, "hash/blake2b-512");
digest_prim!(prim_blake2s256, Blake2s256, "hash/blake2s-256");

// ---------------------------------------------------------------------------
// BLAKE3 one-shot (own API, not RustCrypto Digest)
// ---------------------------------------------------------------------------

fn prim_blake3(args: &[Value]) -> (SignalBits, Value) {
    match oneshot_args(args, "hash/blake3") {
        Ok(data) => (
            SIG_OK,
            Value::bytes(blake3::hash(&data).as_bytes().to_vec()),
        ),
        Err(e) => e,
    }
}

fn prim_blake3_keyed(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "hash/blake3-keyed: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let key = match extract_bytes(&args[0], "hash/blake3-keyed", "key") {
        Ok(d) => d,
        Err(e) => return e,
    };
    if key.len() != 32 {
        return (
            SIG_ERROR,
            error_val(
                "value-error",
                format!(
                    "hash/blake3-keyed: key must be exactly 32 bytes, got {}",
                    key.len()
                ),
            ),
        );
    }
    let data = match extract_bytes(&args[1], "hash/blake3-keyed", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let key_arr: [u8; 32] = key.try_into().unwrap();
    (
        SIG_OK,
        Value::bytes(blake3::keyed_hash(&key_arr, &data).as_bytes().to_vec()),
    )
}

fn prim_blake3_derive(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "hash/blake3-derive: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let context = match extract_string(&args[0], "hash/blake3-derive", "context") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let data = match extract_bytes(&args[1], "hash/blake3-derive", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    (
        SIG_OK,
        Value::bytes(blake3::derive_key(&context, &data).to_vec()),
    )
}

// ---------------------------------------------------------------------------
// CRC32 and xxHash one-shot (return integers or bytes)
// ---------------------------------------------------------------------------

fn prim_crc32(args: &[Value]) -> (SignalBits, Value) {
    match oneshot_args(args, "hash/crc32") {
        Ok(data) => {
            let mut h = Crc32::new();
            h.update(&data);
            (SIG_OK, Value::int(h.finalize() as i64))
        }
        Err(e) => e,
    }
}

fn prim_xxh32(args: &[Value]) -> (SignalBits, Value) {
    match oneshot_args(args, "hash/xxh32") {
        Ok(data) => (
            SIG_OK,
            Value::int(xxhash_rust::xxh32::xxh32(&data, 0) as i64),
        ),
        Err(e) => e,
    }
}

fn prim_xxh64(args: &[Value]) -> (SignalBits, Value) {
    match oneshot_args(args, "hash/xxh64") {
        Ok(data) => (SIG_OK, Value::int(xxhash_rust::xxh3::xxh3_64(&data) as i64)),
        Err(e) => e,
    }
}

fn prim_xxh128(args: &[Value]) -> (SignalBits, Value) {
    match oneshot_args(args, "hash/xxh128") {
        Ok(data) => (
            SIG_OK,
            Value::bytes(xxhash_rust::xxh3::xxh3_128(&data).to_be_bytes().to_vec()),
        ),
        Err(e) => e,
    }
}

// ---------------------------------------------------------------------------
// hex and algorithms
// ---------------------------------------------------------------------------

/// Shared list of algorithm keyword names, used by both make_hasher and prim_algorithms.
const ALGORITHM_NAMES: &[&str] = &[
    "md5",
    "sha1",
    "sha224",
    "sha256",
    "sha384",
    "sha512",
    "sha512-224",
    "sha512-256",
    "sha3-224",
    "sha3-256",
    "sha3-384",
    "sha3-512",
    "blake2b-512",
    "blake2s-256",
    "blake3",
    "crc32",
    "xxh32",
    "xxh64",
    "xxh128",
];

fn prim_hex(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("hash/hex: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let kw = match args[0].as_keyword_name() {
        Some(k) => k,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "hash/hex: first argument must be a keyword, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let data = match extract_bytes(&args[1], "hash/hex", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    // Route through HasherState for consistency with streaming API
    match make_hasher(&kw) {
        Ok(mut state) => {
            state.update(&data);
            let digest = state.finalize_reset();
            // For integer results (crc32, xxh32, xxh64), format as hex string directly
            if let Some(b) = digest.as_bytes() {
                let hex: String = b.iter().map(|byte| format!("{:02x}", byte)).collect();
                (SIG_OK, Value::string(hex.as_str()))
            } else {
                // Integer result — format as hex
                let n = digest.as_int().unwrap_or(0);
                let hex = format!("{:x}", n);
                (SIG_OK, Value::string(hex.as_str()))
            }
        }
        Err(e) => e,
    }
}

fn prim_algorithms(_args: &[Value]) -> (SignalBits, Value) {
    use std::collections::BTreeSet;
    let set: BTreeSet<Value> = ALGORITHM_NAMES
        .iter()
        .map(|name| Value::keyword(name))
        .collect();
    (SIG_OK, Value::set(set))
}

// ---------------------------------------------------------------------------
// Streaming API — HasherState enum + new / update / finalize
// ---------------------------------------------------------------------------

/// Macro to dispatch a method call across all HasherState variants.
macro_rules! dispatch {
    ($self:expr, $h:ident => $body:expr) => {
        match $self {
            HasherState::Md5($h) => $body,
            HasherState::Sha1($h) => $body,
            HasherState::Sha224($h) => $body,
            HasherState::Sha256($h) => $body,
            HasherState::Sha384($h) => $body,
            HasherState::Sha512($h) => $body,
            HasherState::Sha512_224($h) => $body,
            HasherState::Sha512_256($h) => $body,
            HasherState::Sha3_224($h) => $body,
            HasherState::Sha3_256($h) => $body,
            HasherState::Sha3_384($h) => $body,
            HasherState::Sha3_512($h) => $body,
            HasherState::Blake2b512($h) => $body,
            HasherState::Blake2s256($h) => $body,
            HasherState::Blake3($h) => $body,
            HasherState::Crc32($h) => $body,
            HasherState::Xxh32($h) => $body,
            HasherState::Xxh64($h) => $body,
            HasherState::Xxh3($h) => $body,
        }
    };
}

enum HasherState {
    Md5(Md5),
    Sha1(Sha1),
    Sha224(Sha224),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
    Sha512_224(Sha512_224),
    Sha512_256(Sha512_256),
    Sha3_224(Sha3_224),
    Sha3_256(Sha3_256),
    Sha3_384(Sha3_384),
    Sha3_512(Sha3_512),
    Blake2b512(Blake2b512),
    Blake2s256(Blake2s256),
    Blake3(Box<blake3::Hasher>),
    Crc32(Crc32),
    Xxh32(xxhash_rust::xxh32::Xxh32),
    Xxh64(xxhash_rust::xxh64::Xxh64),
    Xxh3(xxhash_rust::xxh3::Xxh3Default),
}

impl HasherState {
    fn update(&mut self, data: &[u8]) {
        dispatch!(self, h => { h.update(data); });
    }

    /// Finalize and reset to a fresh hasher of the same algorithm.
    fn finalize_reset(&mut self) -> Value {
        // The 14 Digest-compatible types all share finalize_reset → bytes.
        // BLAKE3, CRC32, and xxHash each need custom finalize + reset logic.
        match self {
            Self::Blake3(h) => {
                let r = h.finalize();
                h.reset();
                Value::bytes(r.as_bytes().to_vec())
            }
            Self::Crc32(h) => {
                let r = h.clone().finalize();
                h.reset();
                Value::int(r as i64)
            }
            Self::Xxh32(h) => {
                let r = h.digest();
                *h = xxhash_rust::xxh32::Xxh32::new(0);
                Value::int(r as i64)
            }
            Self::Xxh64(h) => {
                let r = h.digest();
                *h = xxhash_rust::xxh64::Xxh64::new(0);
                Value::int(r as i64)
            }
            Self::Xxh3(h) => {
                let r = h.digest128();
                *h = xxhash_rust::xxh3::Xxh3Default::new();
                Value::bytes(r.to_be_bytes().to_vec())
            }
            // All Digest types: finalize_reset returns GenericArray → to_vec
            Self::Md5(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha1(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha224(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha256(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha384(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha512(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha512_224(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha512_256(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha3_224(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha3_256(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha3_384(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Sha3_512(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Blake2b512(h) => Value::bytes(h.finalize_reset().to_vec()),
            Self::Blake2s256(h) => Value::bytes(h.finalize_reset().to_vec()),
        }
    }
}

/// Create a hasher from an algorithm keyword string.
fn make_hasher(kw: &str) -> Result<HasherState, (SignalBits, Value)> {
    match kw {
        "md5" => Ok(HasherState::Md5(Md5::new())),
        "sha1" => Ok(HasherState::Sha1(Sha1::new())),
        "sha224" => Ok(HasherState::Sha224(Sha224::new())),
        "sha256" => Ok(HasherState::Sha256(Sha256::new())),
        "sha384" => Ok(HasherState::Sha384(Sha384::new())),
        "sha512" => Ok(HasherState::Sha512(Sha512::new())),
        "sha512-224" => Ok(HasherState::Sha512_224(Sha512_224::new())),
        "sha512-256" => Ok(HasherState::Sha512_256(Sha512_256::new())),
        "sha3-224" => Ok(HasherState::Sha3_224(Sha3_224::new())),
        "sha3-256" => Ok(HasherState::Sha3_256(Sha3_256::new())),
        "sha3-384" => Ok(HasherState::Sha3_384(Sha3_384::new())),
        "sha3-512" => Ok(HasherState::Sha3_512(Sha3_512::new())),
        "blake2b-512" => Ok(HasherState::Blake2b512(Blake2b512::new())),
        "blake2s-256" => Ok(HasherState::Blake2s256(Blake2s256::new())),
        "blake3" => Ok(HasherState::Blake3(Box::new(blake3::Hasher::new()))),
        "crc32" => Ok(HasherState::Crc32(Crc32::new())),
        "xxh32" => Ok(HasherState::Xxh32(xxhash_rust::xxh32::Xxh32::new(0))),
        "xxh64" => Ok(HasherState::Xxh64(xxhash_rust::xxh64::Xxh64::new(0))),
        "xxh128" => Ok(HasherState::Xxh3(xxhash_rust::xxh3::Xxh3Default::new())),
        _ => Err((
            SIG_ERROR,
            error_val(
                "value-error",
                format!("hash/new: unknown algorithm :{}", kw),
            ),
        )),
    }
}

fn prim_hash_new(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("hash/new: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let kw = match args[0].as_keyword_name() {
        Some(k) => k,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("hash/new: expected keyword, got {}", args[0].type_name()),
                ),
            )
        }
    };
    match make_hasher(&kw) {
        Ok(state) => (SIG_OK, Value::external("hash/context", RefCell::new(state))),
        Err(e) => e,
    }
}

fn prim_hash_update(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("hash/update: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let cell = match args[0].as_external::<RefCell<HasherState>>() {
        Some(c) => c,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "hash/update: first argument must be a hash context, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let data = match extract_bytes(&args[1], "hash/update", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    cell.borrow_mut().update(&data);
    (SIG_OK, args[0])
}

fn prim_hash_finalize(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("hash/finalize: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let cell = match args[0].as_external::<RefCell<HasherState>>() {
        Some(c) => c,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "hash/finalize: expected a hash context, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    (SIG_OK, cell.borrow_mut().finalize_reset())
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "hash/md5",
        func: prim_md5,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "MD5 hash. Returns 16 bytes. Not cryptographically secure.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/md5 \"hello\"))",
        aliases: &["md5"],
    },
    PrimitiveDef {
        name: "hash/sha1",
        func: prim_sha1,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-1 hash. Returns 20 bytes. Not cryptographically secure.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha1 \"hello\"))",
        aliases: &["sha1"],
    },
    PrimitiveDef {
        name: "hash/sha224",
        func: prim_sha224,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-224 hash. Returns 28 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha224 \"hello\"))",
        aliases: &["sha224"],
    },
    PrimitiveDef {
        name: "hash/sha256",
        func: prim_sha256,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-256 hash. Returns 32 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha256 \"hello\"))",
        aliases: &["sha256"],
    },
    PrimitiveDef {
        name: "hash/sha384",
        func: prim_sha384,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-384 hash. Returns 48 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha384 \"hello\"))",
        aliases: &["sha384"],
    },
    PrimitiveDef {
        name: "hash/sha512",
        func: prim_sha512,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-512 hash. Returns 64 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha512 \"hello\"))",
        aliases: &["sha512"],
    },
    PrimitiveDef {
        name: "hash/sha512-224",
        func: prim_sha512_224,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-512/224 hash. Returns 28 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha512-224 \"hello\"))",
        aliases: &["sha512-224"],
    },
    PrimitiveDef {
        name: "hash/sha512-256",
        func: prim_sha512_256,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA-512/256 hash. Returns 32 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha512-256 \"hello\"))",
        aliases: &["sha512-256"],
    },
    PrimitiveDef {
        name: "hash/sha3-224",
        func: prim_sha3_224,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA3-224 (Keccak). Returns 28 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha3-224 \"hello\"))",
        aliases: &["sha3-224"],
    },
    PrimitiveDef {
        name: "hash/sha3-256",
        func: prim_sha3_256,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA3-256 (Keccak). Returns 32 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha3-256 \"hello\"))",
        aliases: &["sha3-256"],
    },
    PrimitiveDef {
        name: "hash/sha3-384",
        func: prim_sha3_384,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA3-384 (Keccak). Returns 48 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha3-384 \"hello\"))",
        aliases: &["sha3-384"],
    },
    PrimitiveDef {
        name: "hash/sha3-512",
        func: prim_sha3_512,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "SHA3-512 (Keccak). Returns 64 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/sha3-512 \"hello\"))",
        aliases: &["sha3-512"],
    },
    PrimitiveDef {
        name: "hash/blake2b-512",
        func: prim_blake2b512,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "BLAKE2b-512 hash. Returns 64 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/blake2b-512 \"hello\"))",
        aliases: &["blake2b-512"],
    },
    PrimitiveDef {
        name: "hash/blake2s-256",
        func: prim_blake2s256,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "BLAKE2s-256 hash. Returns 32 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/blake2s-256 \"hello\"))",
        aliases: &["blake2s-256"],
    },
    PrimitiveDef {
        name: "hash/blake3",
        func: prim_blake3,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "BLAKE3 hash. Returns 32 bytes. Very fast.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/blake3 \"hello\"))",
        aliases: &["blake3"],
    },
    PrimitiveDef {
        name: "hash/blake3-keyed",
        func: prim_blake3_keyed,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "BLAKE3 keyed hash (MAC). Key must be exactly 32 bytes. Returns 32 bytes.",
        params: &["key", "data"],
        category: "hash",
        example: "(bytes->hex (hash/blake3-keyed (hash/blake3 \"mykey\") \"hello\"))",
        aliases: &["blake3-keyed"],
    },
    PrimitiveDef {
        name: "hash/blake3-derive",
        func: prim_blake3_derive,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "BLAKE3 key derivation. Context string + input keying material. Returns 32 bytes.",
        params: &["context", "data"],
        category: "hash",
        example: "(bytes->hex (hash/blake3-derive \"myapp 2026\" \"secret\"))",
        aliases: &["blake3-derive"],
    },
    PrimitiveDef {
        name: "hash/crc32",
        func: prim_crc32,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "CRC32 checksum. Returns an integer.",
        params: &["data"],
        category: "hash",
        example: "(hash/crc32 \"hello\")",
        aliases: &["crc32"],
    },
    PrimitiveDef {
        name: "hash/xxh32",
        func: prim_xxh32,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "xxHash 32-bit. Returns an integer.",
        params: &["data"],
        category: "hash",
        example: "(hash/xxh32 \"hello\")",
        aliases: &["xxh32"],
    },
    PrimitiveDef {
        name: "hash/xxh64",
        func: prim_xxh64,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "xxHash3 64-bit. Returns an integer.",
        params: &["data"],
        category: "hash",
        example: "(hash/xxh64 \"hello\")",
        aliases: &["xxh64"],
    },
    PrimitiveDef {
        name: "hash/xxh128",
        func: prim_xxh128,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "xxHash3 128-bit. Returns 16 bytes.",
        params: &["data"],
        category: "hash",
        example: "(bytes->hex (hash/xxh128 \"hello\"))",
        aliases: &["xxh128"],
    },
    PrimitiveDef {
        name: "hash/hex",
        func: prim_hex,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Hash data and return hex string. (hash/hex :sha256 \"hello\")",
        params: &["algorithm", "data"],
        category: "hash",
        example: "(hash/hex :sha256 \"hello\")",
        aliases: &["hex"],
    },
    PrimitiveDef {
        name: "hash/algorithms",
        func: prim_algorithms,
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return the set of supported algorithm keywords.",
        params: &[],
        category: "hash",
        example: "(hash/algorithms)",
        aliases: &["algorithms"],
    },
    PrimitiveDef {
        name: "hash/new",
        func: prim_hash_new,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create an incremental hasher. Algorithm keyword: :md5, :sha256, :blake3, etc.",
        params: &["algorithm"],
        category: "hash",
        example: "(hash/new :sha256)",
        aliases: &["hash-new"],
    },
    PrimitiveDef {
        name: "hash/update",
        func: prim_hash_update,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Feed data into a hash context. Returns context for stream/fold chaining.",
        params: &["context", "data"],
        category: "hash",
        example: "(hash/update ctx \"hello\")",
        aliases: &["hash-update"],
    },
    PrimitiveDef {
        name: "hash/finalize",
        func: prim_hash_finalize,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Finalize hash context, return digest. Resets context for reuse.",
        params: &["context"],
        category: "hash",
        example: "(bytes->hex (hash/finalize ctx))",
        aliases: &["hash-finalize"],
    },
];
