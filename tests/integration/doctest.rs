// A literate document that `make doctest` runs must be able to reach the end of
// itself.
//
// A document is a program, so a plugin it cannot load is a runtime condition,
// and the sanctioned answer to an unloadable optional dependency is to gate
// (docs/testing.md § "Gating, not skip-lists"). A gate exits 0. So a document
// naming a plugin nothing builds is reported as passing by every `make doctest`
// job on every platform, while none of the Elle below the import runs — the
// coverage disappears and nothing anywhere fails.
//
// These tests are the standing check on the two ways that happens: the plugin
// is never built, or it is named by a path that only one build profile has.
// Both cost a read of the Makefile and a walk of docs/.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// One Makefile rule: what it needs first, and what it runs.
struct Rule {
    prerequisites: Vec<String>,
    recipe: Vec<String>,
}

/// Every rule in the Makefile, by target name.
///
/// A rule opens on an unindented line whose target ends at the first `:`, and
/// its recipe is the tab-indented run that follows. Assignments (`:=`, `?=`,
/// `+=`) and the unindented `define`/`endef` walls that separate the canned
/// recipes therefore close a rule rather than extending one.
fn makefile_rules() -> BTreeMap<String, Rule> {
    let text = fs::read_to_string(repo_root().join("Makefile")).expect("read the Makefile");
    let mut rules: BTreeMap<String, Rule> = BTreeMap::new();
    let mut current: Option<String> = None;

    for line in text.lines() {
        if line.starts_with('\t') {
            if let Some(target) = &current {
                let rule = rules.get_mut(target).expect("the open rule is recorded");
                rule.recipe.push(line.trim().to_string());
            }
            continue;
        }
        current = None;
        let Some((head, tail)) = line.split_once(':') else {
            continue;
        };
        if tail.starts_with('=') || head.ends_with(['?', '+', '!']) {
            continue; // an assignment, not a rule
        }
        let target = head.trim();
        if target.is_empty() || target.starts_with('.') || target.contains(char::is_whitespace) {
            continue; // .PHONY, .DEFAULT_GOAL, and multi-target rules
        }
        // A trailing `## …` is the `make help` blurb, not a prerequisite.
        let prerequisites = tail
            .split("##")
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect();
        rules.insert(
            target.to_string(),
            Rule {
                prerequisites,
                recipe: Vec::new(),
            },
        );
        current = Some(target.to_string());
    }
    rules
}

/// Every cargo package `make TARGET` builds, following prerequisites.
///
/// The names come from the `-p NAME` of each `cargo` line, which is how this
/// Makefile asks for one package out of the workspace.
fn packages_built_by(target: &str) -> BTreeSet<String> {
    let rules = makefile_rules();
    let mut packages = BTreeSet::new();
    let mut pending = vec![target.to_string()];
    let mut seen = BTreeSet::new();

    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(rule) = rules.get(&name) else {
            continue;
        };
        pending.extend(rule.prerequisites.iter().cloned());
        for line in &rule.recipe {
            let mut words = line.split_whitespace();
            while let Some(word) = words.next() {
                if word == "-p" {
                    if let Some(package) = words.next() {
                        packages.insert(package.to_string());
                    }
                }
            }
        }
    }
    packages
}

/// The documents `make doctest` executes, expanded from the recipe's globs.
///
/// Reading them off the recipe rather than listing them here is the point: a
/// directory added to the gate joins these tests in the same commit, and one
/// dropped from it leaves them.
fn doctest_documents() -> Vec<PathBuf> {
    let rules = makefile_rules();
    let doctest = rules.get("doctest").expect("the Makefile defines `doctest`");
    let listing = doctest
        .recipe
        .iter()
        .find(|line| line.contains("printf") && line.contains("docs/"))
        .expect("the doctest recipe lists its documents through printf");

    let root = repo_root();
    let mut documents = Vec::new();
    for glob in listing.split_whitespace() {
        let Some(directory) = glob.strip_suffix("/*.md") else {
            continue;
        };
        let entries = fs::read_dir(root.join(directory))
            .unwrap_or_else(|e| panic!("the doctest recipe globs {directory}, which {e}"));
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                documents.push(path);
            }
        }
    }
    documents.sort();
    documents
}

/// The Elle a document actually executes: its `lisp`/`elle` fences, with the
/// `#` comments dropped so a commented-out example is not read as a live one.
fn executable_elle(document: &PathBuf) -> String {
    let source = fs::read_to_string(document).expect("read a doctest document");
    let mut out = String::new();
    for line in elle::reader::strip_markdown(&source).lines() {
        let mut in_string = false;
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' if in_string => {
                    out.push(c);
                    if let Some(escaped) = chars.next() {
                        out.push(escaped);
                    }
                    continue;
                }
                '"' => in_string = !in_string,
                '#' if !in_string => break,
                _ => {}
            }
            out.push(c);
        }
        out.push('\n');
    }
    out
}

/// Every string literal in `code` that opens with `opening`, without it.
fn literals_starting_with(code: &str, opening: &str) -> Vec<String> {
    let needle = format!("\"{opening}");
    code.match_indices(&needle)
        .filter_map(|(at, _)| {
            let rest = &code[at + needle.len()..];
            rest.split_once('"').map(|(body, _)| body.to_string())
        })
        .collect()
}

/// The documents, paired with the Elle they run. Asserts the walk found the
/// corpus it expects, so a rewrite that matches nothing fails here instead of
/// passing vacuously.
fn documents_and_code() -> Vec<(PathBuf, String)> {
    let documents = doctest_documents();
    assert!(
        documents.len() > 50,
        "expanded only {} documents from the doctest recipe; the walk is broken, \
         not the gate",
        documents.len()
    );
    documents
        .into_iter()
        .map(|document| {
            let code = executable_elle(&document);
            (document, code)
        })
        .collect()
}

// `make doctest` gates four PR jobs on four platforms, and a document whose
// plugin was never built passes all four having run nothing below its import.
// The counter-factual: drop the `myplugin` prerequisite from the `doctest`
// target and `make doctest` still exits 0 — with docs/cookbook/plugins.md
// failing to import, docs/testing.md gating itself out, and every form after
// each import unexecuted.
#[test]
fn every_plugin_a_literate_document_imports_is_built_by_the_doctest_target() {
    let built = packages_built_by("doctest");
    let root = repo_root();
    let manifest =
        fs::read_to_string(root.join("Cargo.toml")).expect("read the workspace manifest");

    let mut imported = BTreeSet::new();
    for (document, code) in documents_and_code() {
        for plugin in literals_starting_with(&code, "plugin/") {
            let package = format!("elle-{plugin}");
            assert!(
                built.contains(&package),
                "{} imports `plugin/{plugin}`, so it needs {package} in \
                 target/<profile>/. Nothing `make doctest` runs builds it, so the \
                 import fails and every form below it in that document is dead.",
                document.display()
            );
            assert!(
                manifest.contains(&format!("\"demos/{plugin}\"")),
                "{package} is built for the doctest gate but is not a member of the \
                 workspace in Cargo.toml, so its library never lands in the \
                 target/<profile>/ directory `import` searches."
            );
            imported.insert(plugin);
        }
    }

    assert!(
        !imported.is_empty(),
        "no literate document imports a plugin. Either the extraction stopped \
         working or the documents stopped covering plugin loading — teach this \
         test the new shape rather than letting it pass by matching nothing."
    );
}

// The other way the gate goes quiet. `import-file` takes the path as written,
// so `target/release/libX.so` resolves only under a release build; `make
// doctest` on a debug binary loads nothing and the document gates itself out.
// `import` picks the running binary's profile instead (docs/testing.md §
// "Gating, not skip-lists").
#[test]
fn no_literate_document_loads_a_native_library_by_a_written_out_path() {
    for (document, code) in documents_and_code() {
        for (at, _) in code.match_indices("(import-file") {
            let Some((_, rest)) = code[at..].split_once('"') else {
                continue;
            };
            let Some((path, _)) = rest.split_once('"') else {
                continue;
            };
            assert!(
                !path.ends_with(".so") && !path.ends_with(".dylib") && !path.ends_with(".dll"),
                "{} loads `{path}` by path. The path names one build profile, so \
                 the document runs its real body under that profile and gates \
                 itself out under the other. Import it as `plugin/NAME`.",
                document.display()
            );
        }
    }
}
