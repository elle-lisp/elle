// audited: 2026-10-26
//! A scratch directory for an inline test, unique under the platform temp root and removed when it drops.
//!
//! tests/AGENTS.md

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A directory this test owns. The name carries the tag, the process id and a
/// per-process count, so two tests, two threads or two runs never share one.
pub(crate) struct ScratchDir(PathBuf);

impl ScratchDir {
    pub(crate) fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("elle-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        ScratchDir(dir)
    }

    /// A path inside the directory.
    pub(crate) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
