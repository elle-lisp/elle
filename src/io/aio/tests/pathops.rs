//! audited: 2026-11-17
//! File operations by path through the backend: the id each completes under, its answer, and the error that names the path.
//!
//! docs/impl/io/platforms.md

use super::*;
use crate::io::request::FileOp;

/// What a completion answered, read out before the completion is discarded:
/// the values live in the region it hands over.
#[derive(Debug, PartialEq)]
pub(super) enum Answer {
    Text(String),
    List(Vec<String>),
    Bool(bool),
    Int(i64),
    /// A stat struct, each of [`STAT_KEYS`] it holds beside its printed value.
    Struct(std::collections::BTreeMap<&'static str, String>),
    Nil,
}

/// Every key a stat struct holds.
pub(super) const STAT_KEYS: [&str; 18] = [
    "accessed",
    "blksize",
    "blocks",
    "created",
    "dev",
    "file-type",
    "gid",
    "inode",
    "is-dir",
    "is-file",
    "is-symlink",
    "modified",
    "nlinks",
    "permissions",
    "rdev",
    "readonly",
    "size",
    "uid",
];

fn text(v: Value) -> String {
    v.with_string(|s| s.to_string()).expect("a string")
}

/// A value as a test reads it: a string's own text, anything else printed.
fn shown(v: Value) -> String {
    v.with_string(|s| s.to_string())
        .unwrap_or_else(|| format!("{}", v))
}

/// What `v` answers, read out of the region it lives in.
fn answer(v: Value) -> Answer {
    if v.is_nil() {
        return Answer::Nil;
    }
    if let Some(b) = v.as_bool() {
        return Answer::Bool(b);
    }
    if let Some(n) = v.as_int() {
        return Answer::Int(n);
    }
    if let Some(entries) = v.as_struct() {
        return Answer::Struct(
            STAT_KEYS
                .iter()
                .filter_map(|k| {
                    sorted_struct_get(entries, &TableKey::keyword(k)).map(|v| (*k, shown(*v)))
                })
                .collect(),
        );
    }
    match v.list_to_vec() {
        Ok(items) => Answer::List(items.into_iter().map(text).collect()),
        Err(_) => Answer::Text(text(v)),
    }
}

/// Submit `op`, wait for its one completion, and answer what it held: the
/// value, or the error's `:path` and its printed form. The submission files one
/// pending entry under the id it answers, and the completion consumes it.
pub(super) fn run(op: FileOp) -> Result<Answer, (Option<String>, String)> {
    let backend = AsyncBackend::new().unwrap();
    let req = IoRequest::unbounded(IoOp::File(op), Value::NIL);
    let id = backend
        .submit(&req, crate::io::pending::Submitter::for_test())
        .unwrap();
    assert_eq!(
        backend.pending_ids(),
        vec![id],
        "one pending entry, under the id"
    );
    let mut completions = backend.wait(PATIENCE).unwrap();
    assert_eq!(completions.len(), 1, "one operation, one completion");
    let c = completions.pop().unwrap();
    assert_eq!(c.id, id);
    assert!(
        backend.pending_ids().is_empty(),
        "the completion consumed it"
    );
    let out = match &c.result {
        Ok(v) => Ok(answer(*v)),
        Err(e) => {
            let path = e
                .as_struct()
                .and_then(|s| sorted_struct_get(s, &TableKey::keyword("path")).copied())
                .and_then(|p| p.with_string(|s| s.to_string()));
            Err((path, format!("{}", e)))
        }
    };
    c.discard();
    out
}

#[test]
fn a_read_answers_the_text_and_lines_answers_the_lines() {
    crate::value::arena::with_test_region(|| {
        let path = write_temp_file("héllo\nworld\n");
        assert_eq!(
            run(FileOp::Read { path: path.clone() }),
            Ok(Answer::Text("héllo\nworld\n".to_string()))
        );
        assert_eq!(
            run(FileOp::Lines { path: path.clone() }),
            Ok(Answer::List(vec!["héllo".to_string(), "world".to_string()]))
        );
        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn a_write_an_append_and_a_copy_answer_true() {
    crate::value::arena::with_test_region(|| {
        let path = temp_path("write");
        let copy = temp_path("copy");
        let write = FileOp::Write {
            path: path.clone(),
            data: b"ab".to_vec(),
        };
        assert_eq!(run(write), Ok(Answer::Bool(true)));
        let append = FileOp::Append {
            path: path.clone(),
            data: b"cd".to_vec(),
        };
        assert_eq!(run(append), Ok(Answer::Bool(true)));
        let copy_op = FileOp::Copy {
            from: path.clone(),
            to: copy.clone(),
        };
        assert_eq!(run(copy_op), Ok(Answer::Bool(true)));
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "abcd");
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&copy).ok();
    });
}

/// A failure is an `:io-error` whose `:path` names the file, for each
/// operation; a copy names the file it copies.
#[test]
fn a_failed_file_operation_names_its_path() {
    crate::value::arena::with_test_region(|| {
        let missing = format!("{}/missing", temp_path("no-such-dir"));
        let ops = [
            FileOp::Read {
                path: missing.clone(),
            },
            FileOp::Lines {
                path: missing.clone(),
            },
            FileOp::Write {
                path: missing.clone(),
                data: b"x".to_vec(),
            },
            FileOp::Append {
                path: missing.clone(),
                data: b"x".to_vec(),
            },
            FileOp::Copy {
                from: missing.clone(),
                to: temp_path("nowhere"),
            },
            FileOp::Sync {
                path: missing.clone(),
            },
        ];
        for op in ops {
            let kind = op.kind();
            let (path, msg) = run(op).expect_err("a missing path fails");
            assert_eq!(path.as_deref(), Some(missing.as_str()), "{kind:?}: :path");
            assert!(msg.contains("io-error"), "{kind:?}: an :io-error: {msg}");
        }
    });
}

/// `file/read` answers text, so bytes that are not UTF-8 are an error rather
/// than a string that holds them.
#[test]
fn a_read_of_bytes_that_are_not_utf8_is_an_io_error() {
    crate::value::arena::with_test_region(|| {
        let path = temp_path("not-utf8");
        std::fs::write(&path, [0xff, 0xfe, b'a']).unwrap();
        let (named, msg) = run(FileOp::Read { path: path.clone() }).expect_err("not text");
        assert_eq!(named.as_deref(), Some(path.as_str()));
        assert!(msg.contains("io-error"), "an :io-error: {msg}");
        std::fs::remove_file(&path).ok();
    });
}

#[test]
fn a_sync_answers_nil() {
    crate::value::arena::with_test_region(|| {
        let path = write_temp_file("sync me");
        assert_eq!(run(FileOp::Sync { path: path.clone() }), Ok(Answer::Nil));
        std::fs::remove_file(&path).ok();
    });
}
