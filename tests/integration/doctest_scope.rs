// audited: 2026-09-23
// Every document `make doctest` should run is listed, and keeps its Elle in lisp fences.
//
// docs/README.md
//
// Coverage disappears when the recipe does not list a document, or when a
// document keeps its code in a fence the reader skips. Nothing runs that code,
// so nothing reports that it stopped working.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use crate::common::documents::{covered_documents, doctest_documents};
use crate::common::repo_root;

// The trap: a recipe that names directories one glob at a time misses the
// subdirectory added after it was written, and a test that reads the same
// globs misses it too. So the covered set is walked from the tree. The
// counter-factual: drop any one path from the recipe's list and this fails
// naming it.
#[test]
fn the_doctest_target_runs_every_document_it_covers() {
    let listed: BTreeSet<PathBuf> = doctest_documents().into_iter().collect();
    let root = repo_root();
    let missing: Vec<String> = covered_documents()
        .into_iter()
        .filter(|document| !listed.contains(document))
        .map(|document| {
            document
                .strip_prefix(&root)
                .unwrap_or(&document)
                .display()
                .to_string()
        })
        .collect();
    assert!(
        missing.is_empty(),
        "`make doctest` does not run these documents, so nothing checks their \
         lisp fences: {missing:#?}"
    );
}

/// The tags of fences that hold a language other than Elle. A line that opens
/// with `(` is that language's own syntax there: a Rust tuple, a shell
/// subshell. Every other tag — none, `text`, and any tag not named here — is
/// held to the rule that it holds no Elle (docs/README.md § "Only a lisp fence
/// holds Elle code").
const FOREIGN: &[&str] = &[
    "rust", "sh", "bash", "sparql", "json", "toml", "sql", "turtle", "mermaid",
];

/// One fenced block: its tag, the line its opening fence is on, and its body.
struct Fence {
    tag: String,
    line: usize,
    body: Vec<String>,
}

/// Every fenced block in a markdown text. A fence opens on a line whose
/// trimmed text starts with three backticks, and the next such line closes it,
/// so a fence indented under a list item counts too.
fn fences(text: &str) -> Vec<Fence> {
    let mut out = Vec::new();
    let mut open: Option<Fence> = None;
    for (n, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("```") {
            match open.take() {
                Some(fence) => out.push(fence),
                None => {
                    open = Some(Fence {
                        tag: rest.trim().to_string(),
                        line: n + 1,
                        body: Vec::new(),
                    })
                }
            }
            continue;
        }
        if let Some(fence) = open.as_mut() {
            fence.body.push(line.to_string());
        }
    }
    out
}

/// Whether one line of a fence opens an Elle form: `(` followed by the first
/// character of a symbol. A value a program prints opens with something else —
/// `(1 2 3)`, `(:ok 3)`, `{:a 1}`, `[1 2]` — so output passes, and a call such
/// as `(defn f [] 1)` or `(fiber/new f |:yield|)` does not.
fn opens_a_form(line: &str) -> bool {
    let mut chars = line.trim_start().chars();
    if chars.next() != Some('(') {
        return false;
    }
    match chars.next() {
        Some(c) => c.is_alphabetic() || "+-*/<>=!?%&._$^~".contains(c),
        None => false,
    }
}

/// The first line in a fence that opens an Elle form, if the fence's tag holds
/// it to the rule.
fn elle_outside_lisp(fence: &Fence) -> Option<&str> {
    let tag = fence.tag.as_str();
    if tag == "lisp" || tag == "elle" || FOREIGN.contains(&tag) {
        return None;
    }
    fence
        .body
        .iter()
        .map(String::as_str)
        .find(|line| opens_a_form(line))
}

// The detector decides what the gate below reports, so it is pinned on the
// shapes the documents hold. The counter-factual for each: a detector that
// flags printed values would fail every output fence in docs/, and one that
// reads only the fence's first line would pass a fence whose code follows a
// comment.
#[test]
fn the_form_detector_tells_elle_from_output_and_other_languages() {
    let sample = "\
```text
# a comment first
(defn f [] 1)
```
```text
(1 2 3)
(:ok 3)
{:a (fn [] 1)}
LoadConst idx      push constant from pool
```
```
  (fiber/new body |:yield|)
```
```scheme
(define x 1)
```
```rust
(SIG_OK, Value::NIL)
```
```lisp
(def x 1)
```
";
    let found: Vec<(String, bool)> = fences(sample)
        .iter()
        .map(|fence| (fence.tag.clone(), elle_outside_lisp(fence).is_some()))
        .collect();
    assert_eq!(
        found,
        vec![
            ("text".to_string(), true),
            ("text".to_string(), false),
            (String::new(), true),
            ("scheme".to_string(), true),
            ("rust".to_string(), false),
            ("lisp".to_string(), false),
        ]
    );
}

// A fence the reader skips is never run, so Elle in one goes stale with
// nothing to report it. The counter-factual: retag any lisp fence in docs/ as
// ```text, as ```scheme, or with no tag, and this fails naming it, where
// `make doctest` passes having skipped it.
#[test]
fn no_fence_outside_lisp_holds_elle_forms() {
    let root = repo_root();
    let mut offending = Vec::new();
    let mut held = 0usize;
    for document in doctest_documents() {
        let text = fs::read_to_string(&document)
            .unwrap_or_else(|e| panic!("read {}: {e}", document.display()));
        for fence in fences(&text) {
            if fence.tag != "lisp" && fence.tag != "elle" && !FOREIGN.contains(&fence.tag.as_str())
            {
                held += 1;
            }
            if let Some(line) = elle_outside_lisp(&fence) {
                let rel = document.strip_prefix(&root).unwrap_or(&document);
                offending.push(format!(
                    "{}:{} ```{}: {}",
                    rel.display(),
                    fence.line,
                    fence.tag,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        held > 0,
        "no document holds a fence the rule applies to; the scan found nothing \
         to check, so it proved nothing"
    );
    assert!(
        offending.is_empty(),
        "fences that are not lisp hold Elle forms; nothing runs them. Move the \
         code into a lisp fence with the scaffolding it needs, or write a \
         synopsis as inline code (docs/README.md): {offending:#?}"
    );
}
