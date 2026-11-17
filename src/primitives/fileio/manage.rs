// audited: 2026-11-17
//! The file/ primitives that change metadata: delete, mkdir, mktempdir and rename, and the size of a file.
//!
//! docs/io/files.md

use super::*;

/// Delete a file
pub(crate) fn prim_delete_file(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/delete", |path| FileOp::Delete { path })
}

/// Delete a directory (must be empty)
pub(crate) fn prim_delete_directory(
    ctx: &mut NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    path_request(ctx, args, "file/delete-dir", |path| FileOp::DeleteDir {
        path,
        recursive: false,
    })
}

/// Delete a directory and everything under it (need not be empty)
pub(crate) fn prim_delete_directory_all(
    ctx: &mut NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    path_request(ctx, args, "file/delete-dir-all", |path| FileOp::DeleteDir {
        path,
        recursive: true,
    })
}

/// Create a uniquely-named directory under the platform temp root.
///
/// The root is `std::env::temp_dir()`, so `TMPDIR` (Unix) or `%TEMP%`
/// (Windows) redirects it — the seam for pointing scratch space at a
/// tmpfs like /dev/shm without any path appearing in Elle code. The root is
/// read here, on the thread that asks; the worker picks the name.
pub(crate) fn prim_make_temp_directory(
    ctx: &mut NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match std::env::temp_dir().into_os_string().into_string() {
        Ok(root) => file_request(ctx, FileOp::MakeTempDir { root }),
        Err(root) => (
            SIG_ERROR,
            ctx.error(
                "io-error",
                format!(
                    "file/mktempdir: the temp root is not UTF-8: {}",
                    root.to_string_lossy()
                ),
            ),
        ),
    }
}

/// Create a directory
pub(crate) fn prim_create_directory(
    ctx: &mut NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    path_request(ctx, args, "file/mkdir", |path| FileOp::Mkdir {
        path,
        parents: false,
    })
}

/// Create a directory and all parent directories
pub(crate) fn prim_create_directory_all(
    ctx: &mut NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    path_request(ctx, args, "file/mkdir-all", |path| FileOp::Mkdir {
        path,
        parents: true,
    })
}

/// Rename a file
pub(crate) fn prim_rename_file(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match string_pair(ctx, args, "file/rename") {
        Ok((from, to)) => file_request(ctx, FileOp::Rename { from, to }),
        Err(e) => e,
    }
}

/// Get file size in bytes
pub(crate) fn prim_file_size(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    path_request(ctx, args, "file/size", |path| FileOp::Size { path })
}
