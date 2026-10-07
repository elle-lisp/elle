// audited: 2026-10-05
// What the rig's tests share: a scratch directory, a run of the rig binary, and the ratchet's scratch producer.
// rig/overview.md

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod ratchet;

/// A directory under the temp root, unique to one test, removed on drop.
///
/// The name carries the process id and a per-process counter, so two tests in
/// one binary and two runs of the binary never share a directory.
pub struct Scratch(PathBuf);

impl Scratch {
    pub fn new(tag: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("elle-rig-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create the scratch directory");
        Scratch(dir)
    }

    /// Write `body` to `name` in the directory and answer its path.
    pub fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, body).unwrap_or_else(|e| panic!("write {name}: {e}"));
        path
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run the rig with `args` and answer what it left behind.
pub fn rig<S: AsRef<std::ffi::OsStr>>(args: &[S]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elle-rig"))
        .args(args)
        .output()
        .expect("spawn elle-rig")
}

pub fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// The lines of the rig's stdout, trimmed, for a caller that asserts one
/// `key = value` line at a time.
pub fn lines(out: &Output) -> Vec<String> {
    stdout(out).lines().map(|l| l.trim().to_string()).collect()
}
