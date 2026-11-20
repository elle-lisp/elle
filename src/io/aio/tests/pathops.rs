//! audited: 2026-11-20
//! File operations by path on both platforms: the id each completes under, its answer, and the error that names the path.
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

/// What one file operation answered: the value, or the error's `:path` and its
/// printed form.
pub(super) type Answered = Result<Answer, (Option<String>, String)>;

/// A backend on this host's own platform, the ring on a Linux build with the
/// `uring` feature, and one on the thread pool.
pub(super) fn platforms() -> [AsyncBackend; 2] {
    [
        AsyncBackend::new().unwrap(),
        AsyncBackend::new_thread_pool().unwrap(),
    ]
}

/// Run `op` on each platform and answer what both answered. An operation that
/// changes the disk changes it once per platform, so `run` takes one that
/// answers the same the second time.
///
/// The counter-factual: the ring and the pool each build an answer from results
/// of their own. One that dropped a field, or spelled an error its own way,
/// would answer a program two ways by platform.
pub(super) fn run(op: FileOp) -> Answered {
    let [ring, pool] = platforms();
    let first = run_on(&ring, op.clone());
    assert_eq!(run_on(&pool, op), first, "both platforms answer alike");
    first
}

/// Wait for the one completion `id` answers with, for as long as `PATIENCE`.
///
/// The trap: a timed wait on the ring returns after one drain, and an
/// operation that takes several steps answers only at its last. A single wait
/// would come back empty after the first step.
pub(super) fn answer_of(backend: &AsyncBackend, id: SubmissionId) -> Completion {
    let deadline = std::time::Instant::now() + PATIENCE.unwrap();
    let mut completions = Vec::new();
    while completions.is_empty() && std::time::Instant::now() < deadline {
        completions = backend.wait(TICK).unwrap();
    }
    assert_eq!(completions.len(), 1, "one operation, one completion");
    let c = completions.pop().unwrap();
    assert_eq!(c.id, id);
    c
}

/// Submit `op` to `backend`, wait for its one completion, and answer what it
/// held. The submission files one pending entry under the id it answers, and
/// the completion consumes it.
pub(super) fn run_on(backend: &AsyncBackend, op: FileOp) -> Answered {
    let req = IoRequest::unbounded(IoOp::File(op), Value::NIL);
    let id = backend
        .submit(&req, crate::io::pending::Submitter::for_test())
        .unwrap();
    assert_eq!(
        backend.pending_ids(),
        vec![id],
        "one pending entry, under the id"
    );
    let c = answer_of(backend, id);
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

/// A failed sync is an `:io-error` whose `:path` names the file.
#[test]
fn a_failed_sync_names_its_path() {
    crate::value::arena::with_test_region(|| {
        let missing = format!("{}/missing", temp_path("no-such-dir"));
        let (path, msg) = run(FileOp::Sync {
            path: missing.clone(),
        })
        .expect_err("a missing path fails");
        assert_eq!(path.as_deref(), Some(missing.as_str()), ":path");
        assert!(msg.contains("io-error"), "an :io-error: {msg}");
    });
}

#[test]
fn a_sync_answers_nil() {
    crate::value::arena::with_test_region(|| {
        let path = write_temp_file("sync me");
        assert_eq!(run(FileOp::Sync { path: path.clone() }), Ok(Answer::Nil));
        std::fs::remove_file(&path).ok();
        let dir = crate::scratch::ScratchDir::new("sync-dir");
        let synced = run(FileOp::Sync {
            path: dir.as_str().to_string(),
        });
        assert_eq!(synced, Ok(Answer::Nil), "a directory syncs");
    });
}

/// A sync of a fifo answers `EINVAL` at once on either platform. Its open asks
/// without blocking, so no writer is needed, and the stat after it refuses
/// anything but a file or a directory.
///
/// The counter-factual: a blocking open would wait for a writer that never
/// comes, and `run` would hang. A sync that skipped the stat would answer the
/// fifo the way `fsync(2)` does, which differs by kernel.
#[test]
fn a_sync_of_a_fifo_refuses_it_at_once() {
    crate::value::arena::with_test_region(|| {
        let fifo = super::fifo::Fifo::new("sync-fifo");
        let (path, msg) = run(FileOp::Sync {
            path: fifo.0.clone(),
        })
        .expect_err("a fifo does not sync");
        assert_eq!(path.as_deref(), Some(fifo.0.as_str()), ":path");
        assert!(msg.contains("Invalid argument"), "EINVAL: {msg}");
    });
}
