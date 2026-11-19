// audited: 2026-11-19
// The runner's store-path flag is documented once, and the runner spells it once.
//
// docs/test-store.md
//
// The session store is meant to hold every run, so that a query joins this run
// against every earlier one. A flag that points a run at a store of its own
// gives that up. Every document that named the flag taught a reader that a
// fresh store is the ordinary way to run a test, and the copies spread from
// there. So the one document that owns where a store lives names the flag in
// its location table, and nothing else in prose does.

use std::path::PathBuf;
use std::process::Command;

const FLAG: &str = "--db";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every file git lists under the repo-relative `pathspec`, tracked or new.
///
/// A file written a minute ago is swept before it is committed, which is the
/// moment a new copy of the flag would arrive.
fn listed(pathspec: &str) -> Vec<String> {
    let out = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
            pathspec,
        ])
        .current_dir(repo_root())
        .output()
        .expect("git ls-files runs");
    assert!(out.status.success(), "git ls-files {pathspec} failed");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// Every `file:line` in `files` that spells the flag as a word of its own.
///
/// A longer flag that happens to start with the same letters, such as
/// `--dbx`, is a different flag, so the character after the match decides.
fn sites(files: &[String]) -> Vec<String> {
    let root = repo_root();
    let mut found = Vec::new();
    for rel in files {
        let Ok(text) = std::fs::read_to_string(root.join(rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for (at, _) in line.match_indices(FLAG) {
                let next = line[at + FLAG.len()..].chars().next();
                let whole =
                    !next.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
                if whole {
                    found.push(format!("{rel}:{}", n + 1));
                }
            }
        }
    }
    found
}

// The counter-factual: a second document naming the flag reads as a second,
// equally good reason to use it, and no other check fails on a copy that is
// correct.
#[test]
fn only_the_store_document_names_the_store_path_flag() {
    let documents = listed("*.md");
    assert!(
        documents.len() > 100,
        "git listed only {} documents; the sweep is broken, not the tree",
        documents.len()
    );
    let found = sites(&documents);
    assert_eq!(
        found.len(),
        1,
        "`{FLAG}` must appear once in all documentation, in the location table \
         of docs/test-store.md; found {found:#?}"
    );
    assert!(
        found[0].starts_with("docs/test-store.md:"),
        "the one `{FLAG}` in the documentation belongs to docs/test-store.md, \
         which owns where a store lives; found it at {}",
        found[0]
    );
}

// The runner's comments are prose a reader meets on the way to the flag table.
// The table is the one place the runner has to spell the flag.
#[test]
fn the_runner_spells_the_store_path_flag_only_in_its_flag_table() {
    let sources = listed("src/test/*.lisp");
    assert!(
        sources.iter().any(|s| s == "src/test/main.lisp"),
        "git did not list src/test/main.lisp; the sweep is broken, not the tree"
    );
    let found = sites(&sources);
    let table = std::fs::read_to_string(repo_root().join("src/test/main.lisp"))
        .expect("read src/test/main.lisp")
        .lines()
        .position(|l| l.contains(&format!("\"{FLAG}\" [:db ")))
        .map(|i| format!("src/test/main.lisp:{}", i + 1))
        .expect("the flag table in src/test/main.lisp names the store path flag");
    assert_eq!(
        found,
        vec![table],
        "`{FLAG}` must appear in src/test only as its entry in the flag table"
    );
}
