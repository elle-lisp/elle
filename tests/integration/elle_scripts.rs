// Elle test script runner
//
// Discovers and runs all .lisp files in tests/elle/. Each script is
// executed as a separate process. All failures are collected and
// reported together.

use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
fn run_elle_test_scripts() {
    let test_dir = Path::new("tests/elle");
    if !test_dir.exists() {
        return;
    }
    let mut failures = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(test_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension() == Some("lisp".as_ref()))
        .collect();
    entries.sort(); // deterministic order
    for path in &entries {
        let output = Command::new(env!("CARGO_BIN_EXE_elle"))
            .arg(path)
            .output()
            .unwrap();
        if !output.status.success() {
            failures.push(format!(
                "{}\n  stdout: {}\n  stderr: {}",
                path.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} Elle test(s) failed:\n{}",
        failures.len(),
        failures.join("\n---\n")
    );
}
