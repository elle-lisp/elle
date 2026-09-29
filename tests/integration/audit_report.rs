// audited: 2026-09-29
// The audit report and `--next` exit 0 over a queue longer than a pipe buffer.
// docs/impl/audit.md
//
// The trap: the script runs under `set -e -o pipefail`, so a `head` that stops
// reading while the queue is still being written kills the writer with SIGPIPE
// and the script exits 141. A short queue fits in the pipe buffer and hides
// that, so these tests build a queue past 64 KiB.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

/// Enough long-named files that the ranked queue exceeds any pipe buffer.
const FILES: usize = 600;

/// A fixture tree under the platform temp root, removed when the test ends.
struct WideTree(PathBuf);

impl WideTree {
    fn new(tag: &str) -> WideTree {
        let dir = std::env::temp_dir().join(format!(
            "elle-audit-report-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create fixture root");
        let pad = "x".repeat(150);
        for n in 0..FILES {
            fs::write(dir.join(format!("{pad}-{n:04}.md")), "# Title\n\nbody\n")
                .expect("write fixture file");
        }
        WideTree(dir)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/audit"))
            .args(["--root", self.0.to_str().expect("utf-8 path")])
            .args(args)
            .output()
            .expect("run scripts/audit")
    }
}

impl Drop for WideTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_report_exits_zero_on_a_queue_longer_than_a_pipe_buffer() {
    let t = WideTree::new("report");
    let out = t.run(&[]);
    let stdout = String::from_utf8(out.stdout).expect("utf-8 output");

    assert!(
        out.status.success(),
        "the report must exit 0, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains(&format!("{FILES} files want a stamp")),
        "the report counts every file:\n{stdout}"
    );
    assert!(
        stdout.contains("next: scripts/audit --next 20"),
        "the report reaches its last line:\n{stdout}"
    );
}

#[test]
fn next_exits_zero_and_lists_n_files_on_a_long_queue() {
    let t = WideTree::new("next");
    let out = t.run(&["--next", "3"]);
    let stdout = String::from_utf8(out.stdout).expect("utf-8 output");

    assert!(
        out.status.success(),
        "--next must exit 0, got {:?}: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        stdout.lines().count(),
        3,
        "--next 3 lists three files:\n{stdout}"
    );
}
