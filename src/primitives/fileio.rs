// audited: 2026-10-21
//! The file/ primitives that only read or change metadata: their table, stat, lstat and ls.
//!
//! docs/io/files.md
use crate::primitives::def::RegionEffect;
use crate::rich_error;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{TableKey, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) mod content;
mod manage;
pub(crate) use manage::*;

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

// The file/ calls that only read or change metadata. content.rs holds the
// table of those that move contents.
primitive! {
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
}
