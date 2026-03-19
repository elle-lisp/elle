//! File I/O primitives
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, error_val_extra, TableKey, Value};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Read entire file as a string
pub(crate) fn prim_slurp(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("slurp: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::read_to_string(path) {
                Ok(content) => (SIG_OK, Value::string(content)),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("slurp: failed to read '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("slurp: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Write string content to a file (overwrites if exists)
pub(crate) fn prim_spit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("spit: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
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
            error_val(
                "type-error",
                format!("spit: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::write(&path, &content) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val_extra(
                "io-error",
                format!("spit: failed to write '{}': {}", path, e),
                &[("path", Value::string(path.as_str()))],
            ),
        ),
    }
}

/// Append string content to a file
pub(crate) fn prim_append_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("append-file: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
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
            error_val(
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
            return (
                SIG_ERROR,
                error_val_extra(
                    "io-error",
                    format!("append-file: failed to open '{}': {}", path, e),
                    &[("path", Value::string(path.as_str()))],
                ),
            )
        }
    };

    match file.write_all(content.as_bytes()) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val_extra(
                "io-error",
                format!("append-file: failed to write '{}': {}", path, e),
                &[("path", Value::string(path.as_str()))],
            ),
        ),
    }
}

/// Delete a file
pub(crate) fn prim_delete_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("delete-file: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::remove_file(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("delete-file: failed to delete '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("delete-file: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Delete a directory (must be empty)
pub(crate) fn prim_delete_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("delete-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::remove_dir(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("delete-directory: failed to delete '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "delete-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Create a directory
pub(crate) fn prim_create_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("create-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::create_dir(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("create-directory: failed to create '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "create-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Create a directory and all parent directories
pub(crate) fn prim_create_directory_all(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "create-directory-all: expected 1 argument, got {}",
                    args.len()
                ),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::create_dir_all(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("create-directory-all: failed to create '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "create-directory-all: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Rename a file
pub(crate) fn prim_rename_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("rename-file: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let old_path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("rename-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let new_path = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("rename-file: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::rename(&old_path, &new_path) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val_extra(
                "io-error",
                format!("rename-file: failed to rename '{}': {}", old_path, e),
                &[("path", Value::string(old_path.as_str()))],
            ),
        ),
    }
}

/// Copy a file
pub(crate) fn prim_copy_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("copy-file: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let src = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("copy-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let dst = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("copy-file: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::copy(&src, &dst) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val_extra(
                "io-error",
                format!("copy-file: failed to copy '{}': {}", src, e),
                &[("path", Value::string(src.as_str()))],
            ),
        ),
    }
}

/// Get file size in bytes
pub(crate) fn prim_file_size(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("file-size: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::metadata(path) {
                Ok(metadata) => (SIG_OK, Value::int(metadata.len() as i64)),
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("file-size: failed to get size of '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("file-size: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

fn kw(name: &str) -> TableKey {
    TableKey::Keyword(name.to_string())
}

fn system_time_to_value(result: std::io::Result<SystemTime>) -> Value {
    match result {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => Value::float(d.as_secs_f64()),
            Err(_) => Value::NIL,
        },
        Err(_) => Value::NIL,
    }
}

fn file_type_string(meta: &std::fs::Metadata) -> &'static str {
    let ft = meta.file_type();
    if ft.is_file() {
        "file"
    } else if ft.is_dir() {
        "dir"
    } else if ft.is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

#[cfg(unix)]
fn insert_unix_fields(fields: &mut BTreeMap<TableKey, Value>, meta: &std::fs::Metadata) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    fields.insert(
        kw("permissions"),
        Value::int(meta.permissions().mode() as i64),
    );
    fields.insert(kw("uid"), Value::int(meta.uid() as i64));
    fields.insert(kw("gid"), Value::int(meta.gid() as i64));
    fields.insert(kw("nlinks"), Value::int(meta.nlink() as i64));
    fields.insert(kw("inode"), Value::int(meta.ino() as i64));
    fields.insert(kw("dev"), Value::int(meta.dev() as i64));
    fields.insert(kw("rdev"), Value::int(meta.rdev() as i64));
    fields.insert(kw("blocks"), Value::int(meta.blocks() as i64));
    fields.insert(kw("blksize"), Value::int(meta.blksize() as i64));
}

#[cfg(not(unix))]
fn insert_unix_fields(fields: &mut BTreeMap<TableKey, Value>, _meta: &std::fs::Metadata) {
    for name in [
        "permissions",
        "uid",
        "gid",
        "nlinks",
        "inode",
        "dev",
        "rdev",
        "blocks",
        "blksize",
    ] {
        fields.insert(kw(name), Value::NIL);
    }
}

fn build_stat_struct(meta: &std::fs::Metadata) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(kw("accessed"), system_time_to_value(meta.accessed()));
    fields.insert(kw("created"), system_time_to_value(meta.created()));
    fields.insert(kw("file-type"), Value::string(file_type_string(meta)));
    fields.insert(kw("is-dir"), Value::bool(meta.is_dir()));
    fields.insert(kw("is-file"), Value::bool(meta.is_file()));
    fields.insert(kw("is-symlink"), Value::bool(meta.is_symlink()));
    fields.insert(kw("modified"), system_time_to_value(meta.modified()));
    fields.insert(kw("readonly"), Value::bool(meta.permissions().readonly()));
    fields.insert(kw("size"), Value::int(meta.len() as i64));
    insert_unix_fields(&mut fields, meta);
    Value::struct_from(fields)
}

fn stat_impl(
    args: &[Value],
    name: &str,
    metadata_fn: fn(&str) -> std::io::Result<std::fs::Metadata>,
) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 argument, got {}", name, args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match metadata_fn(path) {
                Ok(meta) => (SIG_OK, build_stat_struct(&meta)),
                Err(e) => (
                    SIG_ERROR,
                    error_val("io-error", format!("{}: {}: {}", name, path, e)),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", name, args[0].type_name()),
            ),
        )
    }
}

fn metadata_follow(path: &str) -> std::io::Result<std::fs::Metadata> {
    std::fs::metadata(path)
}

fn metadata_nofollow(path: &str) -> std::io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}

/// Get filesystem metadata for a path (follows symlinks).
pub(crate) fn prim_file_stat(args: &[Value]) -> (SignalBits, Value) {
    stat_impl(args, "file/stat", metadata_follow)
}

/// Get filesystem metadata for a path (does not follow symlinks).
pub(crate) fn prim_file_lstat(args: &[Value]) -> (SignalBits, Value) {
    stat_impl(args, "file/lstat", metadata_nofollow)
}

/// List directory contents
pub(crate) fn prim_list_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("list-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
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
                            items.push(Value::string(name));
                        }
                    }
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            error_val_extra(
                                "io-error",
                                format!("list-directory: error reading '{}': {}", path, e),
                                &[("path", Value::string(path.as_str()))],
                            ),
                        );
                    }
                }
            }
            (SIG_OK, crate::value::list(items))
        }
        Err(e) => (
            SIG_ERROR,
            error_val_extra(
                "io-error",
                format!("list-directory: failed to read '{}': {}", path, e),
                &[("path", Value::string(path.as_str()))],
            ),
        ),
    }
}

/// Read lines from a file and return as a list of strings
pub(crate) fn prim_read_lines(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("read-lines: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::read_to_string(path) {
                Ok(content) => {
                    let lines: Vec<Value> = content
                        .lines()
                        .map(|line| Value::string(line.to_string()))
                        .collect();
                    (SIG_OK, crate::value::list(lines))
                }
                Err(e) => (
                    SIG_ERROR,
                    error_val_extra(
                        "io-error",
                        format!("read-lines: failed to read '{}': {}", path, e),
                        &[("path", Value::string(path))],
                    ),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("read-lines: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Declarative primitive definitions for file I/O operations.
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "file/read",
        func: prim_slurp,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read entire file as a string",
        params: &["path"],
        category: "file",
        example: "(file/read \"data.txt\")",
        aliases: &["slurp"],
    },
    PrimitiveDef {
        name: "file/write",
        func: prim_spit,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Write string content to a file (overwrites if exists)",
        params: &["path", "content"],
        category: "file",
        example: "(file/write \"output.txt\" \"hello\")",
        aliases: &["spit"],
    },
    PrimitiveDef {
        name: "file/append",
        func: prim_append_file,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Append string content to a file",
        params: &["path", "content"],
        category: "file",
        example: "(file/append \"log.txt\" \"new line\")",
        aliases: &["append-file"],
    },
    PrimitiveDef {
        name: "file/delete",
        func: prim_delete_file,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Delete a file",
        params: &["path"],
        category: "file",
        example: "(file/delete \"temp.txt\")",
        aliases: &["delete-file"],
    },
    PrimitiveDef {
        name: "file/delete-dir",
        func: prim_delete_directory,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Delete a directory (must be empty)",
        params: &["path"],
        category: "file",
        example: "(file/delete-dir \"empty-dir\")",
        aliases: &["delete-directory"],
    },
    PrimitiveDef {
        name: "file/mkdir",
        func: prim_create_directory,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory",
        params: &["path"],
        category: "file",
        example: "(file/mkdir \"new-dir\")",
        aliases: &["create-directory"],
    },
    PrimitiveDef {
        name: "file/mkdir-all",
        func: prim_create_directory_all,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a directory and all parent directories",
        params: &["path"],
        category: "file",
        example: "(file/mkdir-all \"a/b/c\")",
        aliases: &["create-directory-all"],
    },
    PrimitiveDef {
        name: "file/rename",
        func: prim_rename_file,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Rename a file",
        params: &["old-path", "new-path"],
        category: "file",
        example: "(file/rename \"old.txt\" \"new.txt\")",
        aliases: &["rename-file"],
    },
    PrimitiveDef {
        name: "file/copy",
        func: prim_copy_file,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Copy a file",
        params: &["src", "dst"],
        category: "file",
        example: "(file/copy \"source.txt\" \"dest.txt\")",
        aliases: &["copy-file"],
    },
    PrimitiveDef {
        name: "file/size",
        func: prim_file_size,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get file size in bytes",
        params: &["path"],
        category: "file",
        example: "(file/size \"data.txt\")",
        aliases: &["file-size"],
    },
    PrimitiveDef {
        name: "file/stat",
        func: prim_file_stat,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (follows symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/stat \"data.txt\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "file/lstat",
        func: prim_file_lstat,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get filesystem metadata as a struct (does not follow symlinks)",
        params: &["path"],
        category: "file",
        example: "(file/lstat \"link.txt\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "file/ls",
        func: prim_list_directory,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "List directory contents",
        params: &["path"],
        category: "file",
        example: "(file/ls \".\")",
        aliases: &["list-directory"],
    },
    PrimitiveDef {
        name: "file/lines",
        func: prim_read_lines,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read lines from a file and return as a list of strings",
        params: &["path"],
        category: "file",
        example: "(file/lines \"data.txt\")",
        aliases: &["read-lines"],
    },
];
