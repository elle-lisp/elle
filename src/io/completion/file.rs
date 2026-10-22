//! audited: 2026-10-21
//! Completion handling for a file operation by path: the answer of each kind, and the `:io-error` that names the path.
//!
//! src/io/AGENTS.md
//! docs/io/files.md

use super::*;
use crate::io::request::FileKind;

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
        FileKind::Read => match String::from_utf8(data) {
            Ok(s) => {
                let answer = birth.alloc().string(s);
                Completion::ok(id, birth, answer)
            }
            Err(e) => file_failed(id, birth, kind, path, e),
        },
        FileKind::Lines => match String::from_utf8(data) {
            Ok(s) => {
                let answer = {
                    let ctx = birth.alloc();
                    let lines: Vec<Value> = s.lines().map(|l| ctx.string(l)).collect();
                    ctx.list(lines)
                };
                Completion::ok(id, birth, answer)
            }
            Err(e) => file_failed(id, birth, kind, path, e),
        },
        FileKind::Write | FileKind::Append | FileKind::Copy => {
            Completion::ok(id, birth, Value::TRUE)
        }
        FileKind::Sync => Completion::ok(id, birth, Value::NIL),
    }
}
