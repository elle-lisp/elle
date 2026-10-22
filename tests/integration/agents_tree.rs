// audited: 2026-10-25
// Which files and directories the index generator reads.
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
fn a_directory_whose_name_starts_with_a_dot_is_left_out() {
    // Counter-factual: a walk that drops `.git` alone lists `.cache`, a tool's
    // directory, and writes an index inside it.
    let t = ScratchDir::new("agents-dot-dir");
    write(t.path(), "docs/one.md", "# One\n\nThe first subject.\n");
    write(
        t.path(),
        "docs/.cache/rules.md",
        "# Rules\n\nA tool's own notes.\n",
    );
    write(
        t.path(),
        "docs/sub/two.md",
        "# Two\n\nThe second subject.\n",
    );

    let index = index(t.path(), "docs");
    assert!(
        index.contains("](sub/AGENTS.md)"),
        "a plain directory is listed:\n{index}"
    );
    assert!(
        !index.contains(".cache"),
        "a dot directory is not listed:\n{index}"
    );

    agents(t.path(), &[]);
    assert!(
        !t.join("docs/.cache/AGENTS.md").exists(),
        "the generator writes no index inside a dot directory"
    );
}

#[test]
fn an_untracked_document_never_reaches_an_index() {
    // The trap this pins: a session keeps a scratch note under an ignored name,
    // and the hook would commit an index that lists it.
    let t = ScratchDir::new("agents-untracked");
    git(t.path(), &["init", "-q"]);
    write(t.path(), ".gitignore", "notes.md\n");
    write(t.path(), "docs/one.md", "# One\n\nThe first subject.\n");
    git(t.path(), &["add", ".gitignore", "docs/one.md"]);
    write(
        t.path(),
        "docs/notes.md",
        "# Notes\n\nAn ignored scratch note.\n",
    );
    write(
        t.path(),
        "docs/draft.md",
        "# Draft\n\nA document nobody added yet.\n",
    );
    write(
        t.path(),
        "docs/scratch/plan.md",
        "# Plan\n\nA directory nobody added yet.\n",
    );

    let index = index(t.path(), "docs");
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
    assert!(
        !index.contains("scratch/"),
        "a directory of untracked documents is not listed:\n{index}"
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
