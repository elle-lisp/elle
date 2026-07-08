//! File I/O primitives
use crate::primitives::def::RegionEffect;
use crate::rich_error;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{TableKey, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

mod manage;
pub(crate) use manage::*;

/// Read entire file as a string
pub(crate) fn prim_slurp(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::read_to_string(path) {
                Ok(content) => (SIG_OK, ctx.string(content)),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("slurp: failed to read '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("slurp: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Write string content to a file (overwrites if exists)
pub(crate) fn prim_spit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("spit: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let content = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("spit: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::write(&path, &content) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => rich_error!(
            ctx,
            "io-error",
            format!("spit: failed to write '{}': {}", path, e),
            path = ctx.string(path.as_str()),
        ),
    }
}

/// Append string content to a file
pub(crate) fn prim_append_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("append-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let content = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("append-file: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => f,
        Err(e) => {
            return rich_error!(
                ctx,
                "io-error",
                format!("append-file: failed to open '{}': {}", path, e),
                path = ctx.string(path.as_str()),
            )
        }
    };

    match file.write_all(content.as_bytes()) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => rich_error!(
            ctx,
            "io-error",
            format!("append-file: failed to write '{}': {}", path, e),
            path = ctx.string(path.as_str()),
        ),
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
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "list-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        );
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

/// Read lines from a file and return as a list of strings
pub(crate) fn prim_read_lines(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::read_to_string(path) {
                Ok(content) => {
                    let lines: Vec<Value> = content.lines().map(|s| ctx.string(s)).collect();
                    (SIG_OK, ctx.list(lines))
                }
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("read-lines: failed to read '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("read-lines: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

// Declarative primitive definitions for file I/O operations.
primitive! {
    "file/read" => prim_slurp {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read entire file as a string",
        params: &["path"],
        category: "file",
        example: "(file/read \"data.txt\")",
        aliases: &["slurp"],
        effect: RegionEffect::Fresh,
    }
    "file/write" => prim_spit {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Write string content to a file (overwrites if exists)",
        params: &["path", "content"],
        category: "file",
        example: "(file/write \"output.txt\" \"hello\")",
        aliases: &["spit"],
        effect: RegionEffect::Immediate,
    }
    "file/append" => prim_append_file {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Append string content to a file",
        params: &["path", "content"],
        category: "file",
        example: "(file/append \"log.txt\" \"new line\")",
        aliases: &["append-file"],
        effect: RegionEffect::Immediate,
    }
    "file/delete" => prim_delete_file {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Delete a file",
        params: &["path"],
        category: "file",
        example: "(file/delete \"temp.txt\")",
        aliases: &["delete-file"],
        effect: RegionEffect::Immediate,
    }
    "file/delete-dir" => prim_delete_directory {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory (must be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir \"empty-dir\")",
        aliases: &["delete-directory"],
        effect: RegionEffect::Immediate,
    }
    "file/delete-dir-all" => prim_delete_directory_all {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory and everything under it (need not be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir-all \"scratch\")",
        aliases: &["delete-directory-all"],
        effect: RegionEffect::Immediate,
    }
    "file/mktempdir" => prim_make_temp_directory {
        signal: Signal::errors(),
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
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory",
        params: &["path"],
        category: "file",
        example: "(file/mkdir \"new-dir\")",
        aliases: &["create-directory"],
        effect: RegionEffect::Immediate,
    }
    "file/mkdir-all" => prim_create_directory_all {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory and all parent directories",
        params: &["path"],
        category: "file",
        example: "(file/mkdir-all \"a/b/c\")",
        aliases: &["create-directory-all"],
        effect: RegionEffect::Immediate,
    }
    "file/rename" => prim_rename_file {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Rename a file",
        params: &["old-path", "new-path"],
        category: "file",
        example: "(file/rename \"old.txt\" \"new.txt\")",
        aliases: &["rename-file"],
        effect: RegionEffect::Immediate,
    }
    "file/copy" => prim_copy_file {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Copy a file",
        params: &["src", "dst"],
        category: "file",
        example: "(file/copy \"source.txt\" \"dest.txt\")",
        aliases: &["copy-file"],
        effect: RegionEffect::Immediate,
    }
    "file/size" => prim_file_size {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get file size in bytes",
        params: &["path"],
        category: "file",
        example: "(file/size \"data.txt\")",
        aliases: &["file-size"],
        effect: RegionEffect::Immediate,
    }
    "file/stat" => prim_file_stat {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (follows symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/stat \"data.txt\")",
        effect: RegionEffect::Fresh,
    }
    "file/lstat" => prim_file_lstat {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (does not follow symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/lstat \"link.txt\")",
        effect: RegionEffect::Fresh,
    }
    "file/ls" => prim_list_directory {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "List directory contents",
        params: &["path"],
        category: "file",
        example: "(file/ls \".\")",
        aliases: &["list-directory"],
        effect: RegionEffect::Fresh,
    }
    "file/lines" => prim_read_lines {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read lines from a file and return as a list of strings",
        params: &["path"],
        category: "file",
        example: "(file/lines \"data.txt\")",
        aliases: &["read-lines"],
        effect: RegionEffect::Fresh,
    }
}
