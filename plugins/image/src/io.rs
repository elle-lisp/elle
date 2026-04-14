//! Image I/O primitives: read, write, decode, encode.

use std::io::Cursor;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, Value};

use crate::{get_image, parse_format, require_string, wrap_image};

// ── image/read ──────────────────────────────────────────────────────

fn prim_read(args: &[Value]) -> (SignalBits, Value) {
    let path = match require_string(&args[0], "image/read", "path") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match image::open(&path) {
        Ok(img) => (SIG_OK, wrap_image(img)),
        Err(e) => (
            SIG_ERROR,
            error_val("image-error", format!("image/read: {}", e)),
        ),
    }
}

pub static READ: PrimitiveDef = PrimitiveDef {
    name: "image/read",
    func: prim_read,
    signal: Signal::errors(),
    arity: Arity::Exact(1),
    doc: "Read an image from a file path. Returns an immutable image.",
    params: &["path"],
    category: "image",
    example: "(image/read \"photo.jpg\")",
    aliases: &[],
};

// ── image/write ─────────────────────────────────────────────────────

fn prim_write(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/write") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let path = match require_string(&args[1], "image/write", "path") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match img.save(&path) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => (
            SIG_ERROR,
            error_val("image-error", format!("image/write: {}", e)),
        ),
    }
}

pub static WRITE: PrimitiveDef = PrimitiveDef {
    name: "image/write",
    func: prim_write,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Write an image to a file. Format is inferred from extension.",
    params: &["img", "path"],
    category: "image",
    example: "(image/write img \"out.png\")",
    aliases: &[],
};

// ── image/decode ────────────────────────────────────────────────────

fn prim_decode(args: &[Value]) -> (SignalBits, Value) {
    let data = if let Some(b) = args[0].as_bytes() {
        b.to_vec()
    } else if let Some(bm) = args[0].as_bytes_mut() {
        bm.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("image/decode: expected bytes, got {}", args[0].type_name()),
            ),
        );
    };
    let fmt = match parse_format(&args[1], "image/decode") {
        Ok(f) => f,
        Err(e) => return e,
    };
    let reader = Cursor::new(data);
    match image::load(reader, fmt) {
        Ok(img) => (SIG_OK, wrap_image(img)),
        Err(e) => (
            SIG_ERROR,
            error_val("image-error", format!("image/decode: {}", e)),
        ),
    }
}

pub static DECODE: PrimitiveDef = PrimitiveDef {
    name: "image/decode",
    func: prim_decode,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Decode an image from bytes with a specified format keyword (:png :jpeg :gif :webp :tiff :bmp :ico :qoi).",
    params: &["bytes", "format"],
    category: "image",
    example: "(image/decode raw-bytes :png)",
    aliases: &[],
};

// ── image/encode ────────────────────────────────────────────────────

fn prim_encode(args: &[Value]) -> (SignalBits, Value) {
    let img = match get_image(&args[0], "image/encode") {
        Ok(i) => i,
        Err(e) => return e,
    };
    let fmt = match parse_format(&args[1], "image/encode") {
        Ok(f) => f,
        Err(e) => return e,
    };
    let mut buf = Cursor::new(Vec::new());
    match img.write_to(&mut buf, fmt) {
        Ok(()) => (SIG_OK, Value::bytes(buf.into_inner())),
        Err(e) => (
            SIG_ERROR,
            error_val("image-error", format!("image/encode: {}", e)),
        ),
    }
}

pub static ENCODE: PrimitiveDef = PrimitiveDef {
    name: "image/encode",
    func: prim_encode,
    signal: Signal::errors(),
    arity: Arity::Exact(2),
    doc: "Encode an image to bytes in the specified format. Returns bytes.",
    params: &["img", "format"],
    category: "image",
    example: "(image/encode img :png)",
    aliases: &[],
};
