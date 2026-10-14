// audited: 2026-10-14
//! The operations a pool worker runs on a file by its path: what each reports for a file, a directory, a missing path and a fifo.
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
