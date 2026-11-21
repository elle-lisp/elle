// audited: 2026-11-20
// `elle rewrite` names each def the epoch 17 rule leaves, and each forward read the epoch 18 rule leaves, an included file's too.
// docs/epochs/bodies.md

use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Tells apart the scratch directories of tests running at once.
static SCRATCH: AtomicUsize = AtomicUsize::new(0);

/// `source` written to a scratch file and given to `elle rewrite --dry-run`:
/// the exit code, standard error, the file's path, and its text afterward.
fn dry_run(source: &str) -> (i32, String, String, String) {
    dry_run_beside(source, &[])
}

/// [`dry_run`], with each of `files`, a name and its text, written beside
/// the file first.
fn dry_run_beside(source: &str, files: &[(&str, &str)]) -> (i32, String, String, String) {
    let dir = std::env::temp_dir().join(format!(
        "elle-rewrite-notes-{}-{}",
        std::process::id(),
        SCRATCH.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    for (name, text) in files {
        std::fs::write(dir.join(name), text).expect("write a file beside it");
    }
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
    // A read before the form reaches the def, so the rule neither moves it
    // into a let nor assigns it.
    let source = "(elle/epoch 16)\n(defn f [c]\n  (g x)\n  (when c (def x 1)))\n";
    let (code, stderr, path, after) = dry_run(source);
    assert_eq!(code, 0, "a note is no error (stderr: {stderr})");
    assert!(
        stderr.contains(&format!("{path}:4:11")),
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

#[test]
fn a_forward_read_the_rule_leaves_is_named_by_line_and_column() {
    let source = "(elle/epoch 17)\n(defn f [] (g))\n(println 1)\n(defn g [] 1)\n";
    let (code, stderr, path, after) = dry_run(source);
    assert_eq!(code, 0, "a note is no error (stderr: {stderr})");
    assert!(
        stderr.contains(&format!("{path}:2:13")) && stderr.contains("'g'"),
        "the note names the read where it stands: {stderr}"
    );
    assert_eq!(after, source, "a dry run writes nothing");
}

#[test]
fn a_forward_read_the_rule_carries_is_not_named() {
    let (code, stderr, _, _) = dry_run("(elle/epoch 17)\n(defn f [] (g))\n(defn g [] 1)\n(f)\n");
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.is_empty(), "a carried read needs no note: {stderr}");
}

// --- a read in a file an include splices ---

/// A file whose function reads `gun`.
const LIB: &str = "(defn shoot [] (gun 1))\n";

#[test]
fn an_included_read_of_a_later_def_is_named_at_the_include() {
    // The counter-factual named nothing, and the compiler then refused the
    // read as an undefined variable inside the included file.
    let source = "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n";
    let (code, stderr, path, after) = dry_run_beside(source, &[("lib.lisp", LIB)]);
    assert_eq!(code, 0, "a note is no error (stderr: {stderr})");
    assert!(
        stderr.contains(&format!("{path}:2:1")) && stderr.contains("'gun'"),
        "the note names the include and the name: {stderr}"
    );
    assert!(
        stderr.contains("lib.lisp:1:17"),
        "the note names where the read stands: {stderr}"
    );
    assert_eq!(after, source, "a dry run writes nothing");
}

#[test]
fn an_included_read_in_a_body_is_named_at_its_include() {
    let source =
        "(elle/epoch 17)\n(defn outer []\n  (include-file \"lib.lisp\")\n  (def gun (make-gun))\n  (shoot))\n";
    let (code, stderr, path, _) = dry_run_beside(source, &[("lib.lisp", LIB)]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stderr.contains(&format!("{path}:3:3")) && stderr.contains("'gun'"),
        "the note names the include in the body: {stderr}"
    );
}

#[test]
fn an_included_read_of_an_earlier_def_is_not_named() {
    let source = "(elle/epoch 17)\n(def gun (make-gun))\n(include-file \"lib.lisp\")\n(shoot)\n";
    let (code, stderr, _, _) = dry_run_beside(source, &[("lib.lisp", LIB)]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.is_empty(), "the read finds the def: {stderr}");
}

#[test]
fn a_name_the_included_file_defines_is_not_named() {
    let lib = "(def gun (fn [n] n))\n(defn shoot [] (gun 1))\n";
    let source = "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n";
    let (code, stderr, _, _) = dry_run_beside(source, &[("lib.lisp", lib)]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stderr.is_empty(),
        "the read finds the file's own def: {stderr}"
    );
}

#[test]
fn an_include_of_a_file_that_is_not_there_is_not_named() {
    // The compile reports the missing file; the rewrite has nothing to read.
    let source = "(elle/epoch 17)\n(include-file \"gone.lisp\")\n(def gun (make-gun))\n";
    let (code, stderr, _, _) = dry_run(source);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stderr.is_empty(), "stderr: {stderr}");
}

#[test]
fn a_read_in_a_file_an_included_file_includes_is_named_at_the_outer_include() {
    let source = "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n";
    let files = [
        ("lib.lisp", "(include-file \"deep.lisp\")\n"),
        ("deep.lisp", LIB),
    ];
    let (code, stderr, path, _) = dry_run_beside(source, &files);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(
        stderr.contains(&format!("{path}:2:1")) && stderr.contains("deep.lisp:1:17"),
        "the note names the outer include and the read: {stderr}"
    );
}

#[test]
fn an_included_file_that_includes_itself_is_read_once() {
    // The counter-factual read the file again at each include of it, and
    // never ended.
    let lib = "(include-file \"lib.lisp\")\n(defn shoot [] (gun 1))\n";
    let source = "(elle/epoch 17)\n(include-file \"lib.lisp\")\n(def gun (make-gun))\n(shoot)\n";
    let (code, stderr, _, _) = dry_run_beside(source, &[("lib.lisp", lib)]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert_eq!(
        stderr.matches("lib.lisp:2:17").count(),
        1,
        "one note names the read: {stderr}"
    );
}
