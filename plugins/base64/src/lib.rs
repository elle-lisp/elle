//! Elle base64 plugin — base64 encoding/decoding via the `base64` crate.

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};
elle::elle_plugin_init!(PRIMITIVES, "base64/");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract byte data from string, @string, bytes, or @bytes.
/// Strings are treated as their UTF-8 encoding.
fn extract_byte_data(val: &Value, name: &str, pos: &str) -> Result<Vec<u8>, (SignalBits, Value)> {
    if let Some(bytes) = val.with_string(|s| s.as_bytes().to_vec()) {
        return Ok(bytes);
    }
    if let Some(s_ref) = val.as_string_mut() {
        return Ok(s_ref.borrow().clone());
    }
    if let Some(b) = val.as_bytes() {
        return Ok(b.to_vec());
    }
    if let Some(blob_ref) = val.as_bytes_mut() {
        return Ok(blob_ref.borrow().clone());
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: {} must be string, @string, bytes, or @bytes, got {}",
                name,
                pos,
                val.type_name()
            ),
        ),
    ))
}

/// Extract a string (immutable or mutable) for decode input.
/// @string is stored as Vec<u8> (UTF-8 bytes); we convert to String.
fn extract_string(val: &Value, name: &str, pos: &str) -> Result<String, (SignalBits, Value)> {
    if let Some(s) = val.with_string(|s| s.to_owned()) {
        return Ok(s);
    }
    if let Some(s_ref) = val.as_string_mut() {
        let bytes = s_ref.borrow().clone();
        return match String::from_utf8(bytes) {
            Ok(s) => Ok(s),
            Err(_) => Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: {} is not valid UTF-8", name, pos),
                ),
            )),
        };
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: {} must be string or @string, got {}",
                name,
                pos,
                val.type_name()
            ),
        ),
    ))
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_base64_encode(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("base64/encode: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "base64/encode", "argument") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let encoded = STANDARD.encode(&data);
    (SIG_OK, Value::string(encoded))
}

fn prim_base64_decode(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("base64/decode: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let s = match extract_string(&args[0], "base64/decode", "argument") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match STANDARD.decode(s.as_bytes()) {
        Ok(decoded) => (SIG_OK, Value::bytes(decoded)),
        Err(e) => (
            SIG_ERROR,
            error_val("base64-error", format!("base64/decode: {}", e)),
        ),
    }
}

fn prim_base64_encode_url(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("base64/encode-url: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "base64/encode-url", "argument") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let encoded = URL_SAFE_NO_PAD.encode(&data);
    (SIG_OK, Value::string(encoded))
}

fn prim_base64_decode_url(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("base64/decode-url: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let s = match extract_string(&args[0], "base64/decode-url", "argument") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match URL_SAFE_NO_PAD.decode(s.as_bytes()) {
        Ok(decoded) => (SIG_OK, Value::bytes(decoded)),
        Err(e) => (
            SIG_ERROR,
            error_val("base64-error", format!("base64/decode-url: {}", e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "base64/encode",
        func: prim_base64_encode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Base64-encode data (standard alphabet). Accepts string, @string, bytes, or @bytes. Returns string.",
        params: &["data"],
        category: "base64",
        example: r#"(base64/encode "hello")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "base64/decode",
        func: prim_base64_decode,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Base64-decode a string (standard alphabet). Accepts string or @string. Returns bytes.",
        params: &["data"],
        category: "base64",
        example: r#"(base64/decode "aGVsbG8=")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "base64/encode-url",
        func: prim_base64_encode_url,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Base64-encode data (URL-safe alphabet, no padding). Accepts string, @string, bytes, or @bytes. Returns string.",
        params: &["data"],
        category: "base64",
        example: r#"(base64/encode-url "hello")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "base64/decode-url",
        func: prim_base64_decode_url,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Base64-decode a string (URL-safe alphabet, no padding). Accepts string or @string. Returns bytes.",
        params: &["data"],
        category: "base64",
        example: r#"(base64/decode-url "aGVsbG8")"#,
        aliases: &[],
    },
];
