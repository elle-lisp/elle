// audited: 2026-10-14
//! The operations a pool worker runs on a file by its path: what each reports for a file, a missing path and a fifo.
//!
//! src/io/AGENTS.md

use super::super::*;
use super::{file_path, Fifo};
use crate::io::request::FileOp;

/// Submit a sync of `path` and answer the result code the worker reports.
fn sync_result(id: u64, path: &str) -> i32 {
    let mut pool = CompletionHub::new();
    pool.submit(
        SubmissionId::from_raw(id),
        PoolOp::File(FileOp::Sync {
            path: path.to_string(),
        }),
        Bounds::prompt(),
    )
    .unwrap();
    let completions = pool
        .wait_pool(Some(5000))
        .expect("the sync must report back rather than park in the kernel");
    assert_eq!(completions.len(), 1, "one sync, one completion");
    assert_eq!(completions[0].id, id);
    completions[0].result_code
}

#[test]
fn syncing_a_file_or_a_directory_reports_success() {
    let dir = file_path("sync-dir");
    std::fs::create_dir(&dir).unwrap();
    let file = format!("{}/data", dir);
    std::fs::write(&file, "test").unwrap();

    assert_eq!(sync_result(20, &file), 0, "a file syncs");
    assert_eq!(sync_result(21, &dir), 0, "a directory syncs");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn syncing_a_missing_path_reports_enoent() {
    let path = format!("{}/nofile", file_path("sync-no-such-dir"));
    assert_eq!(sync_result(22, &path), -libc::ENOENT);
}

/// A fifo is neither a file nor a directory, so its sync fails, and fails at
/// once.
///
/// The trap: `open(2)` of a fifo for reading waits for a writer, and nobody
/// writes this one. A sync that opened its path without `O_NONBLOCK` would
/// park the worker inside the syscall, where no bound and no cancel reach it,
/// and `wait_pool` would come back empty.
#[test]
fn syncing_a_fifo_fails_at_once() {
    let fifo = Fifo::new("sync-fifo");
    let code = sync_result(23, &fifo.0);
    assert!(code < 0, "a fifo cannot be synced, got {}", code);
}

/// Run `op` on a worker and answer the result code and the bytes it reports.
fn run_file_op(id: u64, op: FileOp) -> (i32, Vec<u8>) {
    let mut pool = CompletionHub::new();
    pool.submit(
        SubmissionId::from_raw(id),
        PoolOp::File(op),
        Bounds::uninterruptible(),
    )
    .unwrap();
    let mut completions = pool
        .wait_pool(Some(5000))
        .expect("the operation must report back");
    assert_eq!(completions.len(), 1, "one operation, one completion");
    let c = completions.pop().unwrap();
    assert_eq!(c.id, id);
    (c.result_code, c.data)
}

#[test]
fn a_read_reports_the_files_bytes() {
    let path = file_path("read");
    std::fs::write(&path, "one\ntwo\n").unwrap();
    assert_eq!(
        run_file_op(30, FileOp::Read { path: path.clone() }),
        (0, b"one\ntwo\n".to_vec())
    );
    assert_eq!(
        run_file_op(31, FileOp::Lines { path: path.clone() }),
        (0, b"one\ntwo\n".to_vec()),
        "lines reads the same bytes, and the completion splits them"
    );
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_write_replaces_and_an_append_extends() {
    let path = file_path("write");
    std::fs::write(&path, "old contents").unwrap();
    let write = FileOp::Write {
        path: path.clone(),
        data: b"ab".to_vec(),
    };
    assert_eq!(run_file_op(32, write).0, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ab");
    let append = FileOp::Append {
        path: path.clone(),
        data: b"cd".to_vec(),
    };
    assert_eq!(run_file_op(33, append).0, 0);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "abcd");
    std::fs::remove_file(&path).ok();
}

#[test]
fn a_copy_copies_the_file() {
    let from = file_path("copy-from");
    let to = file_path("copy-to");
    std::fs::write(&from, "copied").unwrap();
    let copy = FileOp::Copy {
        from: from.clone(),
        to: to.clone(),
    };
    assert_eq!(run_file_op(34, copy).0, 0);
    assert_eq!(std::fs::read_to_string(&to).unwrap(), "copied");
    std::fs::remove_file(&from).ok();
    std::fs::remove_file(&to).ok();
}

#[test]
fn a_file_operation_on_a_missing_path_reports_enoent() {
    let missing = format!("{}/nofile", file_path("file-no-such-dir"));
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
            to: file_path("copy-nowhere"),
        },
    ];
    for (i, op) in ops.into_iter().enumerate() {
        let kind = op.kind();
        assert_eq!(
            run_file_op(40 + i as u64, op).0,
            -libc::ENOENT,
            "{kind:?} on a missing path"
        );
    }
}
