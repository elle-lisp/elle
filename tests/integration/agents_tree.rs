// audited: 2026-10-22
// Which files and directories the index generator reads, and how it titles the root index.
//
// docs/impl/agents-index.md
//
// The pre-commit hook runs the generator over the working tree and commits what
// it writes, and CI checks the committed index against a clean checkout. An
// index built from a file only one checkout holds, such as an ignored scratch
// note, passes the hook and fails CI, or names the scratch note in a commit.

use std::path::Path;
use std::process::Command;

use crate::common::ScratchDir;

/// Write `body` at `rel` under `root`, creating its directories.
fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("has a parent")).expect("create fixture dir");
    std::fs::write(&path, body).expect("write fixture file");
}

/// Run git in `dir` with a configuration of its own, so the user's hooks,
/// signing and identity never reach the fixture.
fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Run `scripts/agents --root ROOT` with `args`, answering its stdout.
fn agents(root: &Path, args: &[&str]) -> String {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/agents");
    let out = Command::new(script)
        .args(["--root", root.to_str().expect("utf-8 path")])
        .args(args)
        .output()
        .expect("run scripts/agents");
    assert!(
        out.status.success(),
        "scripts/agents failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 output")
}

/// The index `scripts/agents --print DIR` writes.
fn index(root: &Path, dir: &str) -> String {
    agents(root, &["--print", dir])
}

#[test]
fn the_root_index_is_titled_root_and_links_no_parent() {
    // Counter-factual: a title read off the directory name prints `# .` for the
    // root, and names the checkout's own directory when read off the root path.
    let t = ScratchDir::new("agents-root-title");
    write(t.path(), "README.md", "# Top\n\nThe repository.\n");

    let index = index(t.path(), ".");
    assert_eq!(
        index.lines().next(),
        Some("# root"),
        "the root title:\n{index}"
    );
    assert!(
        !index.contains("Up:"),
        "the root index has no parent to link:\n{index}"
    );
}

#[test]
fn a_directory_whose_name_starts_with_a_dot_is_left_out() {
    let t = ScratchDir::new("agents-dot-dir");
    write(t.path(), "README.md", "# Top\n\nThe repository.\n");
    write(
        t.path(),
        ".github/rules.md",
        "# Rules\n\nThe branch rules.\n",
    );
    write(t.path(), "docs/one.md", "# One\n\nThe first subject.\n");

    let index = index(t.path(), ".");
    assert!(
        index.contains("docs/"),
        "a plain directory is listed:\n{index}"
    );
    assert!(
        !index.contains(".github"),
        "a dot directory is not listed:\n{index}"
    );

    agents(t.path(), &[]);
    assert!(
        !t.join(".github/AGENTS.md").exists(),
        "the generator writes no index inside a dot directory"
    );
}

#[test]
fn an_untracked_document_never_reaches_an_index() {
    // The trap this pins: a session keeps a hand-off note at the root under an
    // ignored name, and the hook would commit an index that lists it.
    let t = ScratchDir::new("agents-untracked");
    git(t.path(), &["init", "-q"]);
    write(t.path(), ".gitignore", "notes.md\n");
    write(t.path(), "one.md", "# One\n\nThe first subject.\n");
    git(t.path(), &["add", ".gitignore", "one.md"]);
    write(
        t.path(),
        "notes.md",
        "# Notes\n\nAn ignored scratch note.\n",
    );
    write(
        t.path(),
        "draft.md",
        "# Draft\n\nA document nobody added yet.\n",
    );

    let index = index(t.path(), ".");
    assert!(
        index.contains("one.md"),
        "a tracked document is listed:\n{index}"
    );
    assert!(
        !index.contains("notes.md"),
        "an ignored document is not listed:\n{index}"
    );
    assert!(
        !index.contains("draft.md"),
        "an untracked document is not listed, so the index matches the commit:\n{index}"
    );
}

#[test]
fn a_tracked_document_gone_from_disk_is_not_listed() {
    // Counter-factual: a generator that lists `git ls-files` alone names a
    // document the working tree has deleted, and links to nothing.
    let t = ScratchDir::new("agents-deleted");
    git(t.path(), &["init", "-q"]);
    write(t.path(), "docs/one.md", "# One\n\nThe first subject.\n");
    write(t.path(), "docs/two.md", "# Two\n\nThe second subject.\n");
    git(t.path(), &["add", "docs"]);
    std::fs::remove_file(t.join("docs/two.md")).expect("delete a tracked document");

    let index = index(t.path(), "docs");
    assert!(
        index.contains("one.md"),
        "the document on disk is listed:\n{index}"
    );
    assert!(
        !index.contains("two.md"),
        "a document gone from disk is not listed:\n{index}"
    );
}

#[test]
fn a_submodule_s_documents_never_reach_an_index() {
    // A submodule is checked out in some trees and empty in others, so an index
    // that read it would differ between a checkout and CI.
    let t = ScratchDir::new("agents-submodule");
    git(t.path(), &["init", "-q"]);
    write(t.path(), "README.md", "# Top\n\nThe repository.\n");
    let inner = t.join("vendor");
    std::fs::create_dir_all(&inner).expect("create the inner repository");
    git(&inner, &["init", "-q"]);
    write(&inner, "guide.md", "# Guide\n\nThe vendored guide.\n");
    git(&inner, &["add", "guide.md"]);
    git(&inner, &["commit", "-q", "-m", "guide"]);
    git(t.path(), &["add", "README.md", "vendor"]);

    let index = index(t.path(), ".");
    assert!(
        index.contains("README.md"),
        "the root document is listed:\n{index}"
    );
    assert!(
        !index.contains("vendor") && !index.contains("Guide"),
        "a submodule and its documents are not listed:\n{index}"
    );
}

#[test]
fn a_root_below_another_repository_reads_every_file() {
    // Counter-factual: a generator that asks git whenever the root sits inside
    // any work tree reads the outer repository's index, which tracks nothing
    // here. It writes an empty index for a fixture under a repository's
    // scratch directory.
    let t = ScratchDir::new("agents-nested-root");
    git(t.path(), &["init", "-q"]);
    let root = t.join("fixture");
    write(&root, "docs/one.md", "# One\n\nThe first subject.\n");

    let index = index(&root, "docs");
    assert!(
        index.contains("one.md"),
        "a root that is not the top of a work tree reads every file:\n{index}"
    );
}
