// audited: 2026-11-17
//! The file/ primitives that read or change metadata, and the request every file/ and path/ call on the disk yields.
//!
//! docs/io/files.md

use crate::io::request::{FileOp, IoOp, IoRequest};
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO};
use crate::value::types::Arity;
use crate::value::Value;

pub(crate) mod content;
mod manage;
pub(crate) use manage::*;

// Each primitive that touches the disk yields its operation to the scheduler,
// which hands it to a pool worker.

/// Yield `op` to the scheduler as a portless request.
pub(crate) fn file_request(ctx: &mut NativeCtx<'_>, op: FileOp) -> (SignalBits, Value) {
    (SIG_IO, IoRequest::portless(ctx, IoOp::File(op)))
}

/// Argument `i` as a string, or the type error that names `name`.
pub(crate) fn string_arg(
    ctx: &mut NativeCtx<'_>,
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

/// Arguments 0 and 1 as strings, or the type error that names `name`.
pub(crate) fn string_pair(
    ctx: &mut NativeCtx<'_>,
    args: &[Value],
    name: &str,
) -> Result<(String, String), (SignalBits, Value)> {
    Ok((
        string_arg(ctx, args, 0, name)?,
        string_arg(ctx, args, 1, name)?,
    ))
}

/// Yield the operation `op` makes of the path in argument 0, or raise the
/// type error that names `name`.
pub(crate) fn path_request(
    ctx: &mut NativeCtx<'_>,
    args: &[Value],
    name: &str,
    op: impl FnOnce(String) -> FileOp,
) -> (SignalBits, Value) {
    match string_arg(ctx, args, 0, name) {
        Ok(path) => file_request(ctx, op(path)),
        Err(e) => e,
    }
}

/// Get filesystem metadata for a path (follows symlinks).
pub(crate) fn prim_file_stat(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/stat", |path| FileOp::Stat {
        path,
        follow: true,
    })
}

/// Get filesystem metadata for a path (does not follow symlinks).
pub(crate) fn prim_file_lstat(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/lstat", |path| FileOp::Stat {
        path,
        follow: false,
    })
}

/// List directory contents
pub(crate) fn prim_list_directory(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/ls", |path| FileOp::List { path })
}

// The file/ calls that read or change metadata. content.rs holds the table of
// those that move contents.
//
// An Opaque answer below stores nothing, and is built at the completion's own
// `Birthplace`, neither this call's region nor an argument's.
primitive! {
    "file/delete" => prim_delete_file {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Delete a file",
        params: &["path"],
        category: "file",
        example: "(file/delete \"temp.txt\")",
        aliases: &["delete-file"],
        effect: RegionEffect::Immediate,
    }
    "file/delete-dir" => prim_delete_directory {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory (must be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir \"empty-dir\")",
        aliases: &["delete-directory"],
        effect: RegionEffect::Immediate,
    }
    "file/delete-dir-all" => prim_delete_directory_all {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory and everything under it (need not be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir-all \"scratch\")",
        aliases: &["delete-directory-all"],
        effect: RegionEffect::Immediate,
    }
    "file/mktempdir" => prim_make_temp_directory {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(0),
        doc: "Create a uniquely-named directory under the platform temp root \
              (TMPDIR on Unix, %TEMP% on Windows) and return its path",
        params: &[],
        category: "file",
        example: "(file/mktempdir)",
        aliases: &["make-temp-directory"],
        effect: RegionEffect::Opaque,
    }
    "file/mkdir" => prim_create_directory {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory",
        params: &["path"],
        category: "file",
        example: "(file/mkdir \"new-dir\")",
        aliases: &["create-directory"],
        effect: RegionEffect::Immediate,
    }
    "file/mkdir-all" => prim_create_directory_all {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory and all parent directories",
        params: &["path"],
        category: "file",
        example: "(file/mkdir-all \"a/b/c\")",
        aliases: &["create-directory-all"],
        effect: RegionEffect::Immediate,
    }
    "file/rename" => prim_rename_file {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(2),
        doc: "Rename a file",
        params: &["old-path", "new-path"],
        category: "file",
        example: "(file/rename \"old.txt\" \"new.txt\")",
        aliases: &["rename-file"],
        effect: RegionEffect::Immediate,
    }
    "file/size" => prim_file_size {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Get file size in bytes",
        params: &["path"],
        category: "file",
        example: "(file/size \"data.txt\")",
        aliases: &["file-size"],
        effect: RegionEffect::Immediate,
    }
    "file/stat" => prim_file_stat {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (follows symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/stat \"data.txt\")",
        effect: RegionEffect::Opaque,
    }
    "file/lstat" => prim_file_lstat {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (does not follow symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/lstat \"link.txt\")",
        effect: RegionEffect::Opaque,
    }
    "file/ls" => prim_list_directory {
        signal: Signal::fs_io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "List directory contents",
        params: &["path"],
        category: "file",
        example: "(file/ls \".\")",
        aliases: &["list-directory"],
        effect: RegionEffect::Opaque,
    }
}
