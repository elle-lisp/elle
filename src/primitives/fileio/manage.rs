use super::*;
use crate::rich_error;

/// Delete a file
pub(crate) fn prim_delete_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::remove_file(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("delete-file: failed to delete '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        type_error!(ctx, args[0], "delete-file", "string")
    }
}

/// Delete a directory (must be empty)
pub(crate) fn prim_delete_directory(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::remove_dir(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("delete-directory: failed to delete '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        type_error!(ctx, args[0], "delete-directory", "string")
    }
}

/// Delete a directory and everything under it (need not be empty)
pub(crate) fn prim_delete_directory_all(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::remove_dir_all(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("delete-directory-all: failed to delete '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        type_error!(ctx, args[0], "delete-directory-all", "string")
    }
}

/// Create a uniquely-named directory under the platform temp root.
///
/// The root is `std::env::temp_dir()`, so `TMPDIR` (Unix) or `%TEMP%`
/// (Windows) redirects it — the seam for pointing scratch space at a
/// tmpfs like /dev/shm without any path appearing in Elle code.
/// Uniqueness is pid + a process-local counter; `create_dir` is the
/// atomic claim, and an `AlreadyExists` (a recycled pid meeting a
/// leftover from a dead process) just advances the counter.
pub(crate) fn prim_make_temp_directory(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let base = std::env::temp_dir();
    let pid = std::process::id();
    for _ in 0..1024 {
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let candidate = base.join(format!("elle-{pid}-{n}"));
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                let Some(path) = candidate.to_str() else {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "io-error",
                            format!(
                                "mktempdir: temp root is not UTF-8: '{}'",
                                candidate.display()
                            ),
                        ),
                    );
                };
                return (SIG_OK, ctx.string(path));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "io-error",
                        format!(
                            "mktempdir: failed to create '{}': {}",
                            candidate.display(),
                            e
                        ),
                    ),
                );
            }
        }
    }
    (
        SIG_ERROR,
        ctx.error(
            "io-error",
            format!(
                "mktempdir: exhausted unique-name attempts under '{}'",
                base.display()
            ),
        ),
    )
}

/// Create a directory
pub(crate) fn prim_create_directory(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::create_dir(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("create-directory: failed to create '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        type_error!(ctx, args[0], "create-directory", "string")
    }
}

/// Create a directory and all parent directories
pub(crate) fn prim_create_directory_all(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::create_dir_all(path) {
                Ok(_) => (SIG_OK, Value::TRUE),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("create-directory-all: failed to create '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        type_error!(ctx, args[0], "create-directory-all", "string")
    }
}

/// Rename a file
pub(crate) fn prim_rename_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let old_path = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "rename-file", "string");
    };

    let new_path = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[1], "rename-file", "string");
    };

    match std::fs::rename(&old_path, &new_path) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => rich_error!(
            ctx,
            "io-error",
            format!("rename-file: failed to rename '{}': {}", old_path, e),
            path = ctx.string(old_path.as_str()),
        ),
    }
}

/// Copy a file
pub(crate) fn prim_copy_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let src = if let Some(s) = args[0].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[0], "copy-file", "string");
    };

    let dst = if let Some(s) = args[1].with_string(|s| s.to_string()) {
        s
    } else {
        return type_error!(ctx, args[1], "copy-file", "string");
    };

    match std::fs::copy(&src, &dst) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => rich_error!(
            ctx,
            "io-error",
            format!("copy-file: failed to copy '{}': {}", src, e),
            path = ctx.string(src.as_str()),
        ),
    }
}

/// Get file size in bytes
pub(crate) fn prim_file_size(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match std::fs::metadata(path) {
                Ok(metadata) => (SIG_OK, Value::int(metadata.len() as i64)),
                Err(e) => rich_error!(
                    ctx,
                    "io-error",
                    format!("file-size: failed to get size of '{}': {}", path, e),
                    path = ctx.string(path),
                ),
            })
            .unwrap()
    } else {
        type_error!(ctx, args[0], "file-size", "string")
    }
}

pub(super) fn kw(name: &str) -> TableKey {
    TableKey::keyword(name)
}

pub(super) fn system_time_to_value(result: std::io::Result<SystemTime>) -> Value {
    match result {
        Ok(t) => match t.duration_since(UNIX_EPOCH) {
            Ok(d) => Value::float(d.as_secs_f64()),
            Err(_) => Value::NIL,
        },
        Err(_) => Value::NIL,
    }
}

pub(super) fn file_type_string(meta: &std::fs::Metadata) -> &'static str {
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
pub(super) fn insert_unix_fields(fields: &mut BTreeMap<TableKey, Value>, meta: &std::fs::Metadata) {
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
pub(super) fn insert_unix_fields(
    fields: &mut BTreeMap<TableKey, Value>,
    _meta: &std::fs::Metadata,
) {
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

pub(super) fn build_stat_struct(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    meta: &std::fs::Metadata,
) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(kw("accessed"), system_time_to_value(meta.accessed()));
    fields.insert(kw("created"), system_time_to_value(meta.created()));
    fields.insert(kw("file-type"), ctx.string(file_type_string(meta)));
    fields.insert(kw("is-dir"), Value::bool(meta.is_dir()));
    fields.insert(kw("is-file"), Value::bool(meta.is_file()));
    fields.insert(kw("is-symlink"), Value::bool(meta.is_symlink()));
    fields.insert(kw("modified"), system_time_to_value(meta.modified()));
    fields.insert(kw("readonly"), Value::bool(meta.permissions().readonly()));
    fields.insert(kw("size"), Value::int(meta.len() as i64));
    insert_unix_fields(&mut fields, meta);
    ctx.struct_from(fields)
}

pub(super) fn stat_impl(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
    name: &str,
    metadata_fn: fn(&str) -> std::io::Result<std::fs::Metadata>,
) -> (SignalBits, Value) {
    if args[0].is_string() {
        args[0]
            .with_string(|path| match metadata_fn(path) {
                Ok(meta) => (SIG_OK, build_stat_struct(ctx, &meta)),
                Err(e) => (
                    SIG_ERROR,
                    ctx.error("io-error", format!("{}: {}: {}", name, path, e)),
                ),
            })
            .unwrap()
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{}: expected string, got {}", name, args[0].type_name()),
            ),
        )
    }
}

pub(super) fn metadata_follow(path: &str) -> std::io::Result<std::fs::Metadata> {
    std::fs::metadata(path)
}

pub(super) fn metadata_nofollow(path: &str) -> std::io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}
