// audited: 2026-11-17
//! Completion handling for a file operation by path: the answer of each kind, and the `:io-error` that names the path.
//!
//! docs/impl/io/platforms.md
//! docs/io/files.md

use super::*;
use crate::io::request::{FileKind, StatRecord};

/// A file operation by path that failed with `why`: an `:io-error` whose
/// `:path` names the file.
fn file_failed(
    id: SubmissionId,
    mut birth: crate::io::Birthplace,
    kind: FileKind,
    path: &str,
    why: impl std::fmt::Display,
) -> Completion {
    let error = {
        let ctx = birth.alloc();
        let msg = format!("{}: {}: {}", kind.primitive(), path, why);
        let named = ctx.string(path);
        ctx.error_extra("io-error", msg, &[("path", named)])
    };
    Completion::err(id, birth, error)
}

/// Cook the completion of the file operation `kind` on `path`.
pub(super) fn complete_file_op(
    id: SubmissionId,
    result_code: i32,
    data: Vec<u8>,
    kind: FileKind,
    path: &str,
    mut birth: crate::io::Birthplace,
) -> Completion {
    if result_code < 0 {
        // A worker sends the error's own text; a sync sends none.
        let why = if data.is_empty() {
            std::io::Error::from_raw_os_error(-result_code).to_string()
        } else {
            String::from_utf8_lossy(&data).into_owned()
        };
        return file_failed(id, birth, kind, path, why);
    }
    match kind {
        FileKind::Write
        | FileKind::Append
        | FileKind::Copy
        | FileKind::Mkdir { .. }
        | FileKind::Delete
        | FileKind::DeleteDir { .. }
        | FileKind::Rename => Completion::ok(id, birth, Value::TRUE),
        FileKind::Sync => Completion::ok(id, birth, Value::NIL),
        // A probe answers in its code alone: 1 or 0.
        FileKind::Probe(_) => Completion::ok(id, birth, Value::bool(result_code > 0)),
        FileKind::Size => match <[u8; 8]>::try_from(data.as_slice()) {
            Ok(le) => Completion::ok(id, birth, Value::int(u64::from_le_bytes(le) as i64)),
            Err(_) => file_failed(id, birth, kind, path, "the worker's answer is not a size"),
        },
        FileKind::Stat { .. } => match StatRecord::decode(&data) {
            Some(rec) => {
                let answer = stat_struct(&birth.alloc(), &rec);
                Completion::ok(id, birth, answer)
            }
            None => file_failed(id, birth, kind, path, "the worker's answer is not a stat"),
        },
        FileKind::Read | FileKind::MakeTempDir | FileKind::Canonicalize => {
            match String::from_utf8(data) {
                Ok(s) => {
                    let answer = birth.alloc().string(s);
                    Completion::ok(id, birth, answer)
                }
                Err(e) => file_failed(id, birth, kind, path, e),
            }
        }
        FileKind::Lines | FileKind::List => match String::from_utf8(data) {
            Ok(s) => {
                let answer = {
                    let ctx = birth.alloc();
                    // A list's worker ends each name with a NUL, which no name
                    // holds.
                    let parts: Vec<&str> = match kind {
                        FileKind::List => s.split_terminator('\0').collect(),
                        _ => s.lines().collect(),
                    };
                    ctx.list(parts.into_iter().map(|p| ctx.string(p)))
                };
                Completion::ok(id, birth, answer)
            }
            Err(e) => file_failed(id, birth, kind, path, e),
        },
    }
}

/// The struct `file/stat` and `file/lstat` answer, built from `rec` alone.
fn stat_struct(ctx: &crate::primitives::ctx::Alloc, rec: &StatRecord) -> Value {
    use crate::io::request::FileType;
    let time = |t: Option<f64>| t.map_or(Value::NIL, Value::float);
    let int = |n: u64| Value::int(n as i64);
    let fields = [
        ("accessed", time(rec.accessed)),
        ("created", time(rec.created)),
        ("modified", time(rec.modified)),
        ("file-type", ctx.string(rec.file_type.name())),
        ("is-dir", Value::bool(rec.file_type == FileType::Dir)),
        ("is-file", Value::bool(rec.file_type == FileType::File)),
        (
            "is-symlink",
            Value::bool(rec.file_type == FileType::Symlink),
        ),
        ("readonly", Value::bool(rec.readonly())),
        ("size", int(rec.size)),
        ("permissions", int(rec.permissions)),
        ("uid", int(rec.uid)),
        ("gid", int(rec.gid)),
        ("nlinks", int(rec.nlinks)),
        ("inode", int(rec.inode)),
        ("dev", int(rec.dev)),
        ("rdev", int(rec.rdev)),
        ("blocks", int(rec.blocks)),
        ("blksize", int(rec.blksize)),
    ];
    ctx.struct_from(
        fields
            .into_iter()
            .map(|(k, v)| (TableKey::keyword(k), v))
            .collect(),
    )
}
