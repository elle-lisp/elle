// audited: 2026-11-17
//! The operations a pool worker runs on a path's metadata, and what each reports.
//!
//! docs/io/files.md
//! docs/impl/io/platforms.md

use std::sync::atomic::{AtomicU64, Ordering};

/// The number the next temporary directory of this process takes.
pub(super) static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// The path the `n`th temporary directory of this process takes under `root`.
pub(super) fn temp_candidate(root: &str, n: u64) -> String {
    crate::path::join(&[root, &format!("elle-{}-{}", std::process::id(), n)])
}

/// Make a directory under `root` with a name nothing holds, taking each
/// candidate's number from `counter`.
pub(super) fn make_temp_dir(root: &str, counter: &AtomicU64) -> (i32, Vec<u8>) {
    let _ = temp_candidate(root, counter.load(Ordering::Relaxed));
    (-libc::ENOSYS, Vec::new())
}
