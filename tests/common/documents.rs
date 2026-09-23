// audited: 2026-09-23
//! The documents `make doctest` runs, and the documents it must run.
//!
//! docs/README.md
//!
//! Two test files ask about the same list. `doctest.rs` asks whether each
//! document can reach its own end, and `doctest_scope.rs` asks whether the list
//! is whole and whether its documents keep their Elle where it runs. Both read
//! the list through here, so the two answers come from one reading of it.

use std::fs;
use std::path::{Path, PathBuf};

use super::{make_dry_run, repo_root};

/// The documents `make doctest` executes, as `make` expands its recipe.
///
/// Reading them off the dry run rather than listing them here is the point:
/// whatever the recipe runs, these tests read, and a document the recipe drops
/// leaves them in the same commit. The dry run expands `$(shell …)` and
/// `$(wildcard …)`, so this reads the list the recipe hands `parallel`, not the
/// text that builds it. It does not expand a shell glob, so the recipe builds
/// its list through `make`, and a listed path that is not a file fails here.
#[allow(dead_code)]
pub fn doctest_documents() -> Vec<PathBuf> {
    let recipe = make_dry_run("doctest").expect("`make --dry-run doctest` runs");
    let listing = recipe
        .lines()
        .find(|line| line.contains("printf") && line.contains(".md"))
        .expect("the doctest recipe lists its documents through printf");

    let root = repo_root();
    let mut documents: Vec<PathBuf> = listing
        .split_whitespace()
        .map(|word| word.trim_matches(|c| c == '\'' || c == '"'))
        .filter(|word| word.ends_with(".md"))
        .map(|word| root.join(word))
        .collect();
    let unexpanded: Vec<&PathBuf> = documents.iter().filter(|p| !p.is_file()).collect();
    assert!(
        unexpanded.is_empty(),
        "the doctest recipe lists paths that are not files, such as a shell glob \
         `make` cannot expand: {unexpanded:#?}"
    );
    documents.sort();
    documents.dedup();
    assert!(
        documents.len() > 50,
        "expanded only {} documents from the doctest recipe; the walk is broken, \
         not the gate",
        documents.len()
    );
    documents
}

/// Every document `make doctest` must run, read off the tree itself: the three
/// root guides, and every `.md` under `lib/` and `docs/` at any depth
/// (docs/README.md § "These files are programs").
///
/// This is the independent half of the coverage check. It walks directories
/// rather than reading the recipe, so a subdirectory the recipe's globs never
/// named is still found here.
#[allow(dead_code)]
pub fn covered_documents() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    let root = repo_root();
    let mut documents: Vec<PathBuf> = ["README.md", "QUICKSTART.md", "INSTALL.md"]
        .iter()
        .map(|name| root.join(name))
        .collect();
    walk(&root.join("lib"), &mut documents);
    walk(&root.join("docs"), &mut documents);
    documents.sort();
    documents
}
