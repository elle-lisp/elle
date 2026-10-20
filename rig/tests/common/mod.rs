// audited: 2026-10-20
// What the rig's tests share: a scratch directory and tree, a run of the rig binary, and the ratchet's scratch producer.
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

    /// Write `body` to `name` in the directory, creating the directories it
    /// names, and answer its path.
    pub fn write(&self, name: &str, body: &str) -> PathBuf {
        let path = self.0.join(name);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .unwrap_or_else(|e| panic!("create {name}'s directory: {e}"));
        }
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

/// The repository the rig sits in.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the rig sits in the repository")
        .to_path_buf()
}

/// A scratch tree whose `scripts/audit` runs the repository's own over the
/// tree, which is how a tool lists the tree's sources. The wrapper is two
/// lines, so it is a source under any cap and names no document.
pub fn audited_tree(tag: &str) -> Scratch {
    use std::os::unix::fs::PermissionsExt;
    let t = Scratch::new(tag);
    let body = format!(
        "#!/bin/sh\nexec '{}' --root '{}' \"$@\"\n",
        repo().join("scripts/audit").display(),
        t.path().display()
    );
    let path = t.write("scripts/audit", &body);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
        .expect("make scripts/audit executable");
    t
}
