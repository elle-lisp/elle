//! Elle crypto plugin — SHA-2 family hashes and HMAC via the `sha2` and `hmac` crates.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha224, Sha256, Sha384, Sha512, Sha512_224, Sha512_256};
use std::collections::BTreeMap;

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("crypto/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract byte data from a string or bytes value.
/// Strings are treated as their UTF-8 encoding.
fn extract_byte_data(val: &Value, name: &str, pos: &str) -> Result<Vec<u8>, (SignalBits, Value)> {
    if let Some(bytes) = val.with_string(|s| s.as_bytes().to_vec()) {
        Ok(bytes)
    } else if let Some(b) = val.as_bytes() {
        Ok(b.to_vec())
    } else if let Some(blob_ref) = val.as_blob() {
        Ok(blob_ref.borrow().clone())
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be string, bytes, or blob, got {}",
                    name,
                    pos,
                    val.type_name()
                ),
            ),
        ))
    }
}

// ---------------------------------------------------------------------------
// Primitive generators
// ---------------------------------------------------------------------------

macro_rules! hash_primitive {
    ($fn_name:ident, $hasher:ty, $prim_name:expr) => {
        fn $fn_name(args: &[Value]) -> (SignalBits, Value) {
            if args.len() != 1 {
                return (
                    SIG_ERROR,
                    error_val(
                        "arity-error",
                        format!("{}: expected 1 argument, got {}", $prim_name, args.len()),
                    ),
                );
            }
            let data = match extract_byte_data(&args[0], $prim_name, "argument") {
                Ok(d) => d,
                Err(e) => return e,
            };
            let hash = <$hasher>::digest(&data);
            (SIG_OK, Value::bytes(hash.to_vec()))
        }
    };
}

macro_rules! hmac_primitive {
    ($fn_name:ident, $hasher:ty, $prim_name:expr) => {
        fn $fn_name(args: &[Value]) -> (SignalBits, Value) {
            if args.len() != 2 {
                return (
                    SIG_ERROR,
                    error_val(
                        "arity-error",
                        format!("{}: expected 2 arguments, got {}", $prim_name, args.len()),
                    ),
                );
            }
            let key = match extract_byte_data(&args[0], $prim_name, "key") {
                Ok(d) => d,
                Err(e) => return e,
            };
            let message = match extract_byte_data(&args[1], $prim_name, "message") {
                Ok(d) => d,
                Err(e) => return e,
            };
            let mut mac =
                <Hmac<$hasher>>::new_from_slice(&key).expect("HMAC accepts any key length");
            mac.update(&message);
            let result = mac.finalize().into_bytes();
            (SIG_OK, Value::bytes(result.to_vec()))
        }
    };
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

hash_primitive!(prim_sha224, Sha224, "crypto/sha224");
hash_primitive!(prim_sha256, Sha256, "crypto/sha256");
hash_primitive!(prim_sha384, Sha384, "crypto/sha384");
hash_primitive!(prim_sha512, Sha512, "crypto/sha512");
hash_primitive!(prim_sha512_224, Sha512_224, "crypto/sha512-224");
hash_primitive!(prim_sha512_256, Sha512_256, "crypto/sha512-256");

hmac_primitive!(prim_hmac_sha224, Sha224, "crypto/hmac-sha224");
hmac_primitive!(prim_hmac_sha256, Sha256, "crypto/hmac-sha256");
hmac_primitive!(prim_hmac_sha384, Sha384, "crypto/hmac-sha384");
hmac_primitive!(prim_hmac_sha512, Sha512, "crypto/hmac-sha512");
hmac_primitive!(prim_hmac_sha512_224, Sha512_224, "crypto/hmac-sha512-224");
hmac_primitive!(prim_hmac_sha512_256, Sha512_256, "crypto/hmac-sha512-256");

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    // Hash primitives
    PrimitiveDef {
        name: "crypto/sha224",
        func: prim_sha224,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-224 hash. Accepts string, bytes, or blob. Returns 28 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha224 \"hello\"))",
        aliases: &["sha224"],
    },
    PrimitiveDef {
        name: "crypto/sha256",
        func: prim_sha256,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-256 hash. Accepts string, bytes, or blob. Returns 32 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha256 \"hello\"))",
        aliases: &["sha256"],
    },
    PrimitiveDef {
        name: "crypto/sha384",
        func: prim_sha384,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-384 hash. Accepts string, bytes, or blob. Returns 48 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha384 \"hello\"))",
        aliases: &["sha384"],
    },
    PrimitiveDef {
        name: "crypto/sha512",
        func: prim_sha512,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-512 hash. Accepts string, bytes, or blob. Returns 64 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha512 \"hello\"))",
        aliases: &["sha512"],
    },
    PrimitiveDef {
        name: "crypto/sha512-224",
        func: prim_sha512_224,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-512/224 hash (SHA-512 truncated to 224 bits). Accepts string, bytes, or blob. Returns 28 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha512-224 \"hello\"))",
        aliases: &["sha512-224"],
    },
    PrimitiveDef {
        name: "crypto/sha512-256",
        func: prim_sha512_256,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "SHA-512/256 hash (SHA-512 truncated to 256 bits). Accepts string, bytes, or blob. Returns 32 bytes.",
        params: &["data"],
        category: "crypto",
        example: "(bytes->hex (crypto/sha512-256 \"hello\"))",
        aliases: &["sha512-256"],
    },
    // HMAC primitives
    PrimitiveDef {
        name: "crypto/hmac-sha224",
        func: prim_hmac_sha224,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA224. Takes (key, message). Returns 28 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha224 \"key\" \"message\"))",
        aliases: &["hmac-sha224"],
    },
    PrimitiveDef {
        name: "crypto/hmac-sha256",
        func: prim_hmac_sha256,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA256. Takes (key, message). Returns 32 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha256 \"key\" \"message\"))",
        aliases: &["hmac-sha256"],
    },
    PrimitiveDef {
        name: "crypto/hmac-sha384",
        func: prim_hmac_sha384,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA384. Takes (key, message). Returns 48 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha384 \"key\" \"message\"))",
        aliases: &["hmac-sha384"],
    },
    PrimitiveDef {
        name: "crypto/hmac-sha512",
        func: prim_hmac_sha512,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA512. Takes (key, message). Returns 64 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha512 \"key\" \"message\"))",
        aliases: &["hmac-sha512"],
    },
    PrimitiveDef {
        name: "crypto/hmac-sha512-224",
        func: prim_hmac_sha512_224,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA512/224. Takes (key, message). Returns 28 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha512-224 \"key\" \"message\"))",
        aliases: &["hmac-sha512-224"],
    },
    PrimitiveDef {
        name: "crypto/hmac-sha512-256",
        func: prim_hmac_sha512_256,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "HMAC-SHA512/256. Takes (key, message). Returns 32 bytes.",
        params: &["key", "message"],
        category: "crypto",
        example: "(bytes->hex (crypto/hmac-sha512-256 \"key\" \"message\"))",
        aliases: &["hmac-sha512-256"],
    },
];
