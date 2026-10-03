// audited: 2026-09-29
// `Detect Changes` sets `source` for every change a build reads, so no job
// skips one.
//
// docs/analysis/ci.md
// bins/overview.md

use crate::common::repo_root;

fn workflow_text() -> String {
    let path = repo_root().join(".github/workflows/pr.yml");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The extended regular expression `Detect Changes` greps the changed paths
/// with to set `source`.
fn source_pattern(text: &str) -> String {
    let (_, rest) = text
        .split_once("grep -qE '")
        .expect("`Detect Changes` greps the changed paths");
    rest.split_once('\'')
        .map(|(pattern, _)| pattern.to_string())
        .expect("the pattern closes its quote")
}

/// Whether `grep -qE PATTERN` matches `path`, as the workflow's shell asks it.
fn grep_matches(pattern: &str, path: &str) -> bool {
    let mut child = std::process::Command::new("grep")
        .arg("-qE")
        .arg(pattern)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("run grep");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("grep's stdin");
        writeln!(stdin, "{path}").expect("write to grep");
    }
    child.wait().expect("wait for grep").success()
}

// Every job below `Detect Changes` runs only when `source` is set. The
// counter-factual: a pattern that reads `Cargo.` only at the root of the tree
// skips every job for a change to a variant package's manifest or lockfile,
// the change that decides what the MLIR and WASM jobs build.
#[test]
fn a_change_to_a_variant_package_sets_source() {
    let pattern = source_pattern(&workflow_text());
    for path in [
        "bins/mlir/Cargo.toml",
        "bins/wasm/Cargo.lock",
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "Makefile",
    ] {
        assert!(
            grep_matches(&pattern, path),
            "a change to {path} does not set `source`, so no job runs for it: {pattern}"
        );
    }
    assert!(
        !grep_matches(&pattern, "docs/config.md"),
        "a change to prose alone sets `source`; the parse is broken, not the pattern"
    );
}
