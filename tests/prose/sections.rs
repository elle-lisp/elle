// audited: 2026-10-20
// A quoted section a stamped file cites opens a heading of the document it names.
// DOCUMENTATION.md
//
// The policy cites a document whole, because a section pointer goes stale the
// first time somebody edits the document it points into. The pointers already
// in the tree are checked here, so a document that drops a heading or turns it
// into a paragraph lead fails the build instead of misleading the next reader.
//
// Only a quoted title is read. An unquoted one has no end a machine can find,
// so a check that guessed at it would report pointers that resolve.

use super::examined;
use super::read::{repo_root, stamped};
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn every_quoted_section_a_stamped_file_cites_is_a_heading_of_that_document() {
    let root = repo_root();
    let mut stale = Vec::new();
    let mut seen = 0usize;
    for (rel, text) in stamped() {
        seen += 1;
        for (doc, title) in quoted_section_citations(&text) {
            let Some(path) = resolve_document(&root, &rel, &doc) else {
                stale.push(format!("{rel} -> {doc}: no such document"));
                continue;
            };
            let target = fs::read_to_string(&path).unwrap_or_default();
            if !headings(&target).iter().any(|h| h.starts_with(&title)) {
                stale.push(format!("{rel} -> {doc}: no heading opens with {title:?}"));
            }
        }
    }
    examined(seen, "section citation");
    assert!(
        stale.is_empty(),
        "section citations in stamped files that name no heading; cite the \
         document alone: {stale:#?}"
    );
}

/// Every `(document, title)` pair `text` cites as a document path, a section
/// sign and a quoted title. The text is read with its comment markers and
/// string escapes removed and its lines joined, because a citation wraps.
fn quoted_section_citations(text: &str) -> Vec<(String, String)> {
    let joined = joined_prose(text);
    let mut out = Vec::new();
    for (at, _) in joined.match_indices('§') {
        let Some(doc) = document_before(&joined[..at]) else {
            continue;
        };
        let after = joined[at + '§'.len_utf8()..].trim_start();
        let Some(quote) = after.chars().next().filter(|c| matches!(c, '"' | '\'')) else {
            continue;
        };
        let body = &after[1..];
        if let Some(end) = body.find(quote) {
            let title = body[..end].split_whitespace().collect::<Vec<_>>().join(" ");
            if !title.is_empty() {
                out.push((doc, title));
            }
        }
    }
    out
}

/// `text` as one line: each line loses its leading comment markers and a
/// trailing string-continuation backslash, and an escaped quote reads as a
/// quote.
fn joined_prose(text: &str) -> String {
    let mut out = String::new();
    for line in text.lines() {
        let t = line
            .trim_start()
            .trim_start_matches(['/', '!', '#', '*', ';'])
            .trim();
        let t = t.strip_suffix('\\').unwrap_or(t);
        out.push(' ');
        out.push_str(&t.replace("\\\"", "\""));
    }
    out
}

/// The document path that ends just before a section sign, read back over
/// whitespace and the bracket that closes a markdown link.
fn document_before(prefix: &str) -> Option<String> {
    let p = prefix.trim_end();
    let p = p.strip_suffix(')').unwrap_or(p);
    let start = p
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '_' | '-'))
        .last()
        .map(|(i, _)| i)?;
    let doc = &p[start..];
    doc.ends_with(".md").then(|| doc.to_string())
}

/// Where a cited document lives: from the repository root, from the citing
/// file's directory, or under docs/impl, which the region comments abbreviate.
fn resolve_document(root: &Path, citing: &str, doc: &str) -> Option<PathBuf> {
    let dir = root.join(citing);
    let dir = dir.parent().expect("has a parent");
    [
        root.join(doc),
        dir.join(doc),
        root.join("docs/impl").join(doc),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// The heading texts of a markdown document, outside fenced blocks, with their
/// whitespace collapsed.
fn headings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced || !line.starts_with('#') {
            continue;
        }
        let h = line.trim_start_matches('#');
        if h.starts_with(' ') {
            out.push(h.split_whitespace().collect::<Vec<_>>().join(" "));
        }
    }
    out
}
