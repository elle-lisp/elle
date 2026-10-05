// audited: 2026-10-04
// No Rust file but the test runner `src/io/isolate.rs` calls `libc::fork`.
//
// docs/analysis/testing.md
//
// A test that forks the test harness hands its child every lock another test
// thread held at that instant, held for good. The child hangs when it touches
// one, and which locks those are changes with every run. `io::isolate::run`
// forks only inside a fresh copy of the test binary, where no thread holds a
// lock. This sweep is what keeps a test from forking anywhere else.

use crate::common::{repo_root, source_files};

/// The one file that may fork, because it forks only where that is safe.
const RUNNER: &str = "src/io/isolate.rs";

#[test]
fn only_the_isolated_test_runner_calls_fork() {
    // Assembled from parts so this file never matches itself.
    let needle = format!("libc::{}(", "fork");
    let dirs = [
        "src",
        "tests",
        "rig",
        "benches",
        "demos",
        "tools",
        "elle-plugin",
        "bins",
    ];
    let files = source_files(&dirs, &["rs"]);
    assert!(
        files.iter().any(|path| path.ends_with(RUNNER)),
        "the sweep did not read {RUNNER}; it reads the wrong tree"
    );

    let mut offenders = Vec::new();
    for path in files {
        let shown = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .to_path_buf();
        if shown.as_os_str() == RUNNER {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.contains(&needle) {
                offenders.push(format!("{}:{}: {}", shown.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these files fork; run the body through `crate::io::isolate::run` \
         instead (docs/analysis/testing.md):\n{}",
        offenders.join("\n")
    );
}
