// audited: 2026-11-17
// `elle rewrite` names on standard error each def the epoch 17 rule leaves, by file, line and column.
// docs/epochs.md

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Tells apart the scratch directories of tests running at once.
static SCRATCH: AtomicUsize = AtomicUsize::new(0);

/// `source` written to a scratch file and given to `elle rewrite --dry-run`:
/// the exit code, standard error, the file's path, and its text afterward.
fn dry_run(source: &str) -> (i32, String, String, String) {
    let dir = std::env::temp_dir().join(format!(
        "elle-rewrite-notes-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    let file = dir.join("old.lisp");
    std::fs::write(&file, source).expect("write the file");
    let path = file.to_str().expect("utf-8 path").to_string();
    let out = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(["rewrite", "--dry-run", &path])
        .output()
        .expect("spawn elle");
    let after = std::fs::read_to_string(&file).expect("read the file back");
    std::fs::remove_dir_all(&dir).expect("remove scratch dir");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        path,
        after,
    )
}

#[test]
fn a_def_the_rule_leaves_is_named_by_line_and_column() {
    let source = "(elle/epoch 16)\n(defn f [c]\n  (when c (def x 1))\n  x)\n";
    let (code, stderr, path, after) = dry_run(source);
    assert_eq!(code, 0, "a note is no error (stderr: {stderr})");
    assert!(
        stderr.contains(&format!("{path}:3:11")),
        "the note names the def where it stands: {stderr}"
    );
    assert_eq!(after, source, "a dry run writes nothing");
}

#[test]
fn a_def_the_rule_moves_is_not_named() {
    let (code, stderr, _, _) = dry_run("(elle/epoch 16)\n(defn f [c] (when c (def x 1) (g x)))\n");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.is_empty(), "a moved def needs no note: {stderr}");
}
