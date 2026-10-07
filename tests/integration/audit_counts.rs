// audited: 2026-10-05
// The audit queue's two counts, and the walk over the files git tracks.
//
// docs/impl/audit.md
//
// The counts are what tests/ratchet/audit.lisp reads and the ledger pins, so
// a count that differed between two checkouts of one commit would fail a run
// on one box and pass it on another.

use std::path::Path;
use std::process::{Command, Output};

use crate::common::ScratchDir;

fn script() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/audit")
}

/// Write `body` at `rel` under `root`, creating its directories.
fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("has a parent")).expect("create fixture dir");
    std::fs::write(&path, body).expect("write fixture file");
}

/// A document carrying `stamp` if one is given.
fn doc(stamp: Option<&str>) -> String {
    match stamp {
        Some(d) => format!("# Title\n\n<!-- audited: {d} -->\n\nbody\n"),
        None => "# Title\n\nbody\n".to_string(),
    }
}

/// `scripts/audit --root ROOT ARGS…`, which must succeed.
fn audit(root: &Path, args: &[&str]) -> String {
    let out: Output = Command::new(script())
        .args(["--root", root.to_str().expect("utf-8 path")])
        .args(args)
        .output()
        .expect("run scripts/audit");
    assert!(
        out.status.success(),
        "scripts/audit {args:?} failed with {:?}:\n{}{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// `git ARGS…` in `root`, which must succeed.
fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn counts_names_the_unstamped_files_and_the_files_stamped_before_the_policy() {
    // The policy file stamps itself on the floor, which is not before it.
    let t = ScratchDir::new("audit-counts");
    write(t.path(), "DOCUMENTATION.md", &doc(Some("2026-06-01")));
    write(t.path(), "never.md", &doc(None));
    write(t.path(), "also-never.rs", "// no stamp\nfn f() {}\n");
    write(t.path(), "before.md", &doc(Some("2026-05-31")));
    write(t.path(), "current.md", &doc(Some("2026-06-02")));

    assert_eq!(
        audit(t.path(), &["--counts"]),
        "unstamped 2\nbefore-policy 1\n",
        "two files carry no stamp and one predates the policy; no file is in both"
    );
}

#[test]
fn the_walk_lists_only_the_files_git_tracks() {
    // The counter-factual: `find` lists every file under the root, so a
    // hand-off note nobody committed sits in the queue and in the counts, and
    // one commit reads a different count in each checkout.
    let t = ScratchDir::new("audit-tracked");
    write(t.path(), "DOCUMENTATION.md", &doc(Some("2026-06-01")));
    write(t.path(), "tracked.md", &doc(None));
    write(t.path(), "untracked.md", &doc(None));
    git(t.path(), &["init", "-q"]);
    git(t.path(), &["add", "DOCUMENTATION.md", "tracked.md"]);

    let queue = audit(t.path(), &["--all"]);
    assert!(
        queue.contains("tracked.md") && !queue.contains("untracked.md"),
        "the queue holds the tracked file alone:\n{queue}"
    );
    assert_eq!(
        audit(t.path(), &["--counts"]),
        "unstamped 1\nbefore-policy 0\n",
        "and so do the counts"
    );
}

#[test]
fn a_tree_inside_another_repository_has_every_file_walked() {
    // The trap: a fixture directory under a temp root that sits inside some
    // repository is no work tree of its own. Asking git which files it tracks
    // answers the enclosing repository's, which names none of the fixture's,
    // and the queue reads empty.
    let t = ScratchDir::new("audit-nested");
    git(t.path(), &["init", "-q"]);
    write(t.path(), "inner/DOCUMENTATION.md", &doc(Some("2026-06-01")));
    write(t.path(), "inner/a.md", &doc(None));

    assert_eq!(
        audit(&t.path().join("inner"), &["--counts"]),
        "unstamped 1\nbefore-policy 0\n",
        "a root below the top of a work tree walks every file under it"
    );
}
