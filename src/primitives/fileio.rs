// audited: 2026-10-14
//! The file/ primitives: whole-file reads and writes, directories, metadata, and file/sync.
//!
//! docs/io.md
use crate::io::request::{FileOp, IoOp, IoRequest};
use crate::primitives::def::RegionEffect;
use crate::rich_error;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK};
use crate::value::types::Arity;
use crate::value::{TableKey, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

mod manage;
pub(crate) use manage::*;

// The primitives below move a file's contents, so each yields its operation
// to the scheduler, which hands it to a pool worker (docs/io.md).

/// Yield `op` to the scheduler as a portless request.
fn file_request(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    op: FileOp,
) -> (SignalBits, Value) {
    (SIG_IO, IoRequest::portless(ctx, IoOp::File(op)))
}

/// Argument `i` as a string, or the type error that names `name`.
fn string_arg(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
    i: usize,
    name: &str,
) -> Result<String, (SignalBits, Value)> {
    match args[i].with_string(|s| s.to_string()) {
        Some(s) => Ok(s),
        None => Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: expected string, got {}", name, args[i].type_name()),
            ),
        )),
    }
}

/// Read entire file as a string
pub(crate) fn prim_slurp(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match string_arg(ctx, args, 0, "file/read") {
        Ok(path) => file_request(ctx, FileOp::Read { path }),
        Err(e) => e,
    }
}

/// Read lines from a file and return as a list of strings
pub(crate) fn prim_read_lines(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match string_arg(ctx, args, 0, "file/lines") {
        Ok(path) => file_request(ctx, FileOp::Lines { path }),
        Err(e) => e,
    }
}

/// The path and the string content of a write or an append.
fn path_and_data(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
    name: &str,
) -> Result<(String, Vec<u8>), (SignalBits, Value)> {
    let path = string_arg(ctx, args, 0, name)?;
    let data = string_arg(ctx, args, 1, name)?;
    Ok((path, data.into_bytes()))
}

/// Write string content to a file (overwrites if exists)
pub(crate) fn prim_spit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match path_and_data(ctx, args, "file/write") {
        Ok((path, data)) => file_request(ctx, FileOp::Write { path, data }),
        Err(e) => e,
    }
}

/// Append string content to a file
pub(crate) fn prim_append_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match path_and_data(ctx, args, "file/append") {
        Ok((path, data)) => file_request(ctx, FileOp::Append { path, data }),
        Err(e) => e,
    }
}

/// Copy a file
pub(crate) fn prim_copy_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let from = match string_arg(ctx, args, 0, "file/copy") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match string_arg(ctx, args, 1, "file/copy") {
        Ok(to) => file_request(ctx, FileOp::Copy { from, to }),
        Err(e) => e,
    }
}

/// Flush the file or directory at a path to stable storage
pub(crate) fn prim_file_sync(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match string_arg(ctx, args, 0, "file/sync") {
        Ok(path) => file_request(ctx, FileOp::Sync { path }),
        Err(e) => e,
    }
}

/// Get filesystem metadata for a path (follows symlinks).
pub(crate) fn prim_file_stat(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    stat_impl(ctx, args, "file/stat", metadata_follow)
}

/// Get filesystem metadata for a path (does not follow symlinks).
pub(crate) fn prim_file_lstat(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    stat_impl(ctx, args, "file/lstat", metadata_nofollow)
}

/// List directory contents
pub(crate) fn prim_list_directory(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "list-directory", "string");
    };

    match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut items = Vec::new();
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        if let Ok(name) = entry.file_name().into_string() {
                            items.push(ctx.string(name));
                        }
                    }
                    Err(e) => {
                        return rich_error!(
                            ctx,
                            "io-error",
                            format!("list-directory: error reading '{}': {}", path, e),
                            path = ctx.string(path.as_str()),
                        );
                    }
                }
            }
            (SIG_OK, ctx.list(items))
        }
        Err(e) => rich_error!(
            ctx,
            "io-error",
            format!("list-directory: failed to read '{}': {}", path, e),
            path = ctx.string(path.as_str()),
        ),
    }
}

// Declarative primitive definitions for file I/O operations.
primitive! {
    "file/read" => prim_slurp {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Read entire file as a string",
        params: &["path"],
        category: "file",
        example: "(file/read \"data.txt\")",
        aliases: &["slurp"],
        // Opaque: stores nothing, and the answer is built at the completion's own
        // `Birthplace`, neither this call's region nor an argument's.
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
    "file/delete" => prim_delete_file {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Delete a file",
        params: &["path"],
        category: "file",
        example: "(file/delete \"temp.txt\")",
        aliases: &["delete-file"],
        effect: RegionEffect::Immediate,
    }
    "file/delete-dir" => prim_delete_directory {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory (must be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir \"empty-dir\")",
        aliases: &["delete-directory"],
        effect: RegionEffect::Immediate,
    }
    "file/delete-dir-all" => prim_delete_directory_all {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory and everything under it (need not be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir-all \"scratch\")",
        aliases: &["delete-directory-all"],
        effect: RegionEffect::Immediate,
    }
    "file/mktempdir" => prim_make_temp_directory {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(0),
        doc: "Create a uniquely-named directory under the platform temp root \
              (TMPDIR on Unix, %TEMP% on Windows) and return its path",
        params: &[],
        category: "file",
        example: "(file/mktempdir)",
        aliases: &["make-temp-directory"],
        effect: RegionEffect::Fresh,
    }
    "file/mkdir" => prim_create_directory {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory",
        params: &["path"],
        category: "file",
        example: "(file/mkdir \"new-dir\")",
        aliases: &["create-directory"],
        effect: RegionEffect::Immediate,
    }
    "file/mkdir-all" => prim_create_directory_all {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory and all parent directories",
        params: &["path"],
        category: "file",
        example: "(file/mkdir-all \"a/b/c\")",
        aliases: &["create-directory-all"],
        effect: RegionEffect::Immediate,
    }
    "file/rename" => prim_rename_file {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(2),
        doc: "Rename a file",
        params: &["old-path", "new-path"],
        category: "file",
        example: "(file/rename \"old.txt\" \"new.txt\")",
        aliases: &["rename-file"],
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
    "file/size" => prim_file_size {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Get file size in bytes",
        params: &["path"],
        category: "file",
        example: "(file/size \"data.txt\")",
        aliases: &["file-size"],
        effect: RegionEffect::Immediate,
    }
    "file/stat" => prim_file_stat {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (follows symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/stat \"data.txt\")",
        effect: RegionEffect::Fresh,
    }
    "file/lstat" => prim_file_lstat {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (does not follow symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/lstat \"link.txt\")",
        effect: RegionEffect::Fresh,
    }
    "file/ls" => prim_list_directory {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "List directory contents",
        params: &["path"],
        category: "file",
        example: "(file/ls \".\")",
        aliases: &["list-directory"],
        effect: RegionEffect::Fresh,
    }
    "file/lines" => prim_read_lines {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Read lines from a file and return as a list of strings",
        params: &["path"],
        category: "file",
        example: "(file/lines \"data.txt\")",
        aliases: &["read-lines"],
        // Opaque: stores nothing, and the answer is built at the completion's own
        // `Birthplace`, neither this call's region nor an argument's.
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
