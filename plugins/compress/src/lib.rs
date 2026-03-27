//! Elle compress plugin — gzip, zlib, deflate, and zstd compression via flate2 and zstd.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use flate2::read::{DeflateDecoder, GzDecoder, ZlibDecoder};
use flate2::write::{DeflateEncoder, GzEncoder, ZlibEncoder};
use flate2::Compression;

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
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
        let short_name = def.name.strip_prefix("compress/").unwrap_or(def.name);
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

/// Extract an optional compression level from args[1] (if present).
/// Returns the level as u32 if valid, or an error (SignalBits, Value).
/// `range` is the inclusive valid range; `default` is used when arg is absent.
fn extract_level(
    args: &[Value],
    name: &str,
    range: std::ops::RangeInclusive<i64>,
    default: u32,
) -> Result<u32, (SignalBits, Value)> {
    if args.len() < 2 {
        return Ok(default);
    }
    match args[1].as_int() {
        Some(n) if range.contains(&n) => Ok(n as u32),
        Some(n) => Err((
            SIG_ERROR,
            error_val(
                "compress-error",
                format!(
                    "{}: level must be {}–{}, got {}",
                    name,
                    range.start(),
                    range.end(),
                    n
                ),
            ),
        )),
        None => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: level must be integer, got {}",
                    name,
                    args[1].type_name()
                ),
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_gzip(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "compress/gzip: expected 1 or 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/gzip", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let level = match extract_level(args, "compress/gzip", 0..=9, 6) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let mut enc = GzEncoder::new(Vec::new(), Compression::new(level));
    if let Err(e) = enc.write_all(&data) {
        return (
            SIG_ERROR,
            error_val("compress-error", format!("compress/gzip: {}", e)),
        );
    }
    match enc.finish() {
        Ok(out) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/gzip: {}", e)),
        ),
    }
}

fn prim_gunzip(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("compress/gunzip: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/gunzip", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut dec = GzDecoder::new(&data[..]);
    let mut out = Vec::new();
    match dec.read_to_end(&mut out) {
        Ok(_) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/gunzip: {}", e)),
        ),
    }
}

fn prim_deflate(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "compress/deflate: expected 1 or 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/deflate", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let level = match extract_level(args, "compress/deflate", 0..=9, 6) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let mut enc = DeflateEncoder::new(Vec::new(), Compression::new(level));
    if let Err(e) = enc.write_all(&data) {
        return (
            SIG_ERROR,
            error_val("compress-error", format!("compress/deflate: {}", e)),
        );
    }
    match enc.finish() {
        Ok(out) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/deflate: {}", e)),
        ),
    }
}

fn prim_inflate(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("compress/inflate: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/inflate", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut dec = DeflateDecoder::new(&data[..]);
    let mut out = Vec::new();
    match dec.read_to_end(&mut out) {
        Ok(_) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/inflate: {}", e)),
        ),
    }
}

fn prim_zlib(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "compress/zlib: expected 1 or 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/zlib", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let level = match extract_level(args, "compress/zlib", 0..=9, 6) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::new(level));
    if let Err(e) = enc.write_all(&data) {
        return (
            SIG_ERROR,
            error_val("compress-error", format!("compress/zlib: {}", e)),
        );
    }
    match enc.finish() {
        Ok(out) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/zlib: {}", e)),
        ),
    }
}

fn prim_unzlib(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("compress/unzlib: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/unzlib", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut dec = ZlibDecoder::new(&data[..]);
    let mut out = Vec::new();
    match dec.read_to_end(&mut out) {
        Ok(_) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/unzlib: {}", e)),
        ),
    }
}

fn prim_zstd(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "compress/zstd: expected 1 or 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/zstd", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    let level = match extract_level(args, "compress/zstd", 1..=22, 3) {
        Ok(l) => l,
        Err(e) => return e,
    };
    match zstd::encode_all(Cursor::new(&data), level as i32) {
        Ok(out) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/zstd: {}", e)),
        ),
    }
}

fn prim_unzstd(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("compress/unzstd: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let data = match extract_byte_data(&args[0], "compress/unzstd", "data") {
        Ok(d) => d,
        Err(e) => return e,
    };
    match zstd::decode_all(Cursor::new(&data)) {
        Ok(out) => (SIG_OK, Value::bytes(out)),
        Err(e) => (
            SIG_ERROR,
            error_val("compress-error", format!("compress/unzstd: {}", e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "compress/gzip",
        func: prim_gzip,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Gzip-compress data. Optional compression level (0–9, default 6). Returns bytes.",
        params: &["data", "level"],
        category: "compress",
        example: r#"(compress/gzip "hello")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/gunzip",
        func: prim_gunzip,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Gzip-decompress data. Returns bytes.",
        params: &["data"],
        category: "compress",
        example: r#"(compress/gunzip compressed-bytes)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/deflate",
        func: prim_deflate,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Deflate-compress data (raw DEFLATE, no wrapper). Optional level (0–9, default 6). Returns bytes.",
        params: &["data", "level"],
        category: "compress",
        example: r#"(compress/deflate "hello")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/inflate",
        func: prim_inflate,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Inflate-decompress data (raw DEFLATE). Returns bytes.",
        params: &["data"],
        category: "compress",
        example: r#"(compress/inflate compressed-bytes)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/zlib",
        func: prim_zlib,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Zlib-compress data. Optional compression level (0–9, default 6). Returns bytes.",
        params: &["data", "level"],
        category: "compress",
        example: r#"(compress/zlib "hello")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/unzlib",
        func: prim_unzlib,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Zlib-decompress data. Returns bytes.",
        params: &["data"],
        category: "compress",
        example: r#"(compress/unzlib compressed-bytes)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/zstd",
        func: prim_zstd,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Zstd-compress data. Optional compression level (1–22, default 3). Returns bytes.",
        params: &["data", "level"],
        category: "compress",
        example: r#"(compress/zstd "hello")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "compress/unzstd",
        func: prim_unzstd,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Zstd-decompress data. Returns bytes.",
        params: &["data"],
        category: "compress",
        example: r#"(compress/unzstd compressed-bytes)"#,
        aliases: &[],
    },
];
