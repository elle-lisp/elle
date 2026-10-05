// audited: 2026-10-04
// No `.rs` or `.lisp` file in the tree names a hardcoded /tmp path.
//
// tests/AGENTS.md
//
// Temp paths must derive from the platform temp root — std::env::temp_dir()
// in Rust, file/mktempdir / with-temp-dir in Elle — never a hardcoded /tmp
// (shared, size-limited, and not where TMPDIR points). A sweep is the only
// thing that sees a new offender before it ships.

use crate::common::{repo_root, source_files};

#[test]
fn no_hardcoded_tmp_paths() {
    // Assembled from parts so this file never matches itself.
    let needle = format!("\"/{}", "tmp");
    let dirs = [
        "src",
        "lib",
        "tests",
        "tools",
        "demos",
        "benches",
        "elle-plugin",
    ];
    let mut offenders = Vec::new();
    for path in source_files(&dirs, &["rs", "lisp"]) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.contains(&needle) {
                let shown = path.strip_prefix(repo_root()).unwrap_or(&path);
                offenders.push(format!("{}:{}: {}", shown.display(), i + 1, line.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "hardcoded {} paths found ({}); derive from the platform temp root \
         (std::env::temp_dir / file/mktempdir) and clean up after use:\n{}",
        needle,
        offenders.len(),
        offenders.join("\n")
    );
}
