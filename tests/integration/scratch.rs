// Scratch-file policy enforcement.
//
// Temp paths must derive from the platform temp root — std::env::temp_dir()
// in Rust, file/mktempdir / with-temp-dir in Elle — never a hardcoded /tmp
// (shared, size-limited, and not where TMPDIR points). This test sweeps every
// .rs and .lisp file in the tree for a quoted /tmp path so a new offender
// fails CI instead of shipping. See tests/AGENTS.md § Scratch files.

use std::path::{Path, PathBuf};

fn scan(dir: &Path, needle: &str, offenders: &mut Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == ".git" {
                continue;
            }
            scan(&path, needle, offenders);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs") | Some("lisp")
        ) {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for (i, line) in text.lines().enumerate() {
                if line.contains(needle) {
                    offenders.push(format!("{}:{}: {}", path.display(), i + 1, line.trim()));
                }
            }
        }
    }
}

#[test]
fn no_hardcoded_tmp_paths() {
    // Assembled from parts so this file never matches itself.
    let needle = format!("\"/{}", "tmp");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();
    for dir in [
        "src",
        "lib",
        "tests",
        "tools",
        "demos",
        "benches",
        "elle-plugin",
    ] {
        scan(&root.join(dir), &needle, &mut offenders);
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
