// audited: 2026-11-17
//! The file/ primitives that move a file's contents: read, lines, write, append, copy and sync, each run on the I/O pool.
//!
//! docs/io/files.md

use super::{file_request, path_request, string_pair};
use crate::io::request::FileOp;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::types::Arity;
use crate::value::Value;

/// Read entire file as a string
pub(crate) fn prim_slurp(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/read", |path| FileOp::Read { path })
}

/// Read lines from a file and return as a list of strings
pub(crate) fn prim_read_lines(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/lines", |path| FileOp::Lines { path })
}

/// Write string content to a file (overwrites if exists)
pub(crate) fn prim_spit(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match string_pair(ctx, args, "file/write") {
        Ok((path, data)) => file_request(
            ctx,
            FileOp::Write {
                path,
                data: data.into_bytes(),
            },
        ),
        Err(e) => e,
    }
}

/// Append string content to a file
pub(crate) fn prim_append_file(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match string_pair(ctx, args, "file/append") {
        Ok((path, data)) => file_request(
            ctx,
            FileOp::Append {
                path,
                data: data.into_bytes(),
            },
        ),
        Err(e) => e,
    }
}

/// Copy a file
pub(crate) fn prim_copy_file(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match string_pair(ctx, args, "file/copy") {
        Ok((from, to)) => file_request(ctx, FileOp::Copy { from, to }),
        Err(e) => e,
    }
}

/// Flush the file or directory at a path to stable storage
pub(crate) fn prim_file_sync(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/sync", |path| FileOp::Sync { path })
}

// Each Opaque answer below is built where fileio.rs says.
primitive! {
    "file/read" => prim_slurp {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Read entire file as a string",
        params: &["path"],
        category: "file",
        example: "(file/read \"data.txt\")",
        aliases: &["slurp"],
        effect: RegionEffect::Opaque,
    }
    "file/write" => prim_spit {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(2),
        doc: "Write string content to a file (overwrites if exists)",
        params: &["path", "content"],
        category: "file",
        example: "(file/write \"output.txt\" \"hello\")",
        aliases: &["spit"],
        effect: RegionEffect::Immediate,
    }
    "file/append" => prim_append_file {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(2),
        doc: "Append string content to a file",
        params: &["path", "content"],
        category: "file",
        example: "(file/append \"log.txt\" \"new line\")",
        aliases: &["append-file"],
        effect: RegionEffect::Immediate,
    }
    "file/copy" => prim_copy_file {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(2),
        doc: "Copy a file",
        params: &["src", "dst"],
        category: "file",
        example: "(file/copy \"source.txt\" \"dest.txt\")",
        aliases: &["copy-file"],
        effect: RegionEffect::Immediate,
    }
    "file/lines" => prim_read_lines {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Read lines from a file and return as a list of strings",
        params: &["path"],
        category: "file",
        example: "(file/lines \"data.txt\")",
        aliases: &["read-lines"],
        effect: RegionEffect::Opaque,
    }
    "file/sync" => prim_file_sync {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Flush the file or directory at path to stable storage, as fsync(2) \
              does, and answer nil. Other fibers run while it waits for the disk. \
              Raises :io-error for a path that does not exist, that cannot be \
              read, or that names neither a file nor a directory.",
        params: &["path"],
        category: "file",
        example: "(file/sync \"data.txt\")",
        // Immediate: the Sync completion returns Value::NIL.
        effect: RegionEffect::Immediate,
    }
}
