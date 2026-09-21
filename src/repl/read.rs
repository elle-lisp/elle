// audited: 2026-09-20
//! Splitting accumulated prompt input into whole forms, and naming what each
//! one binds.
//!
//! docs/impl/lexicon.md
//!
//! Input arrives a line at a time, so a form can be half-written: the reader's
//! "unterminated" errors are what tell an incomplete form from a broken one,
//! and an incomplete one leaves the accumulated text standing for the next
//! line.

use crate::reader::read_syntax_all_current;
use crate::syntax::{Syntax, SyntaxKind};

/// Result of attempting to parse accumulated input.
pub(super) enum ReadResult {
    /// Input parsed into one or more complete forms.
    Complete(Vec<FormInfo>),
    /// Input is incomplete (unterminated delimiter).
    Incomplete,
    /// Hard parse error.
    Error(String),
}

/// A parsed form with enough metadata to compile it individually.
pub(super) struct FormInfo {
    /// Source text of this form (sliced from accumulated input via span byte offsets).
    pub(super) source: String,
    /// Bindings introduced by this form, if any.
    pub(super) bindings: Vec<DefBinding>,
}

/// A binding introduced by a top-level def/var form.
pub(super) struct DefBinding {
    pub(super) name: String,
}

/// Try to parse source into complete forms.
///
/// Prompt input is always current-epoch (docs/impl/lexicon.md): a pasted
/// `(elle/epoch N)` is a form the session evaluates, never a choice of
/// lexer for the text around it.
pub(super) fn try_read(source: &str) -> ReadResult {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return ReadResult::Incomplete;
    }

    // The prompt's tree is read only to split the input into forms and name
    // their bindings; nothing outlives this call, so it gets its own heap.
    let mut home = crate::syntax::SyntaxHeap::new();
    match read_syntax_all_current(home.arena(), trimmed, "<repl>") {
        Ok(syntaxes) if syntaxes.is_empty() => ReadResult::Incomplete,
        Ok(syntaxes) => {
            let forms = syntaxes
                .iter()
                .map(|syn| FormInfo {
                    source: trimmed[syn.span.start as usize..syn.span.end as usize].to_string(),
                    bindings: extract_def_bindings(syn),
                })
                .collect();
            ReadResult::Complete(forms)
        }
        Err(e) if is_incomplete_error(&e) => ReadResult::Incomplete,
        Err(e) => ReadResult::Error(e),
    }
}

/// Check whether a reader error indicates incomplete input.
fn is_incomplete_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("unterminated") || lower.contains("unexpected end of input")
}

/// Extract binding names from a def/var/defn form.
///
/// Handles:
/// - `(def name ...)` / `(var name ...)` / `(defn name ...)` → one name
/// - `(def [a b] ...)` / `(var [a b] ...)` → leaf names from destructure
/// - `(def {x y} ...)` → leaf names from struct destructure
fn extract_def_bindings(syntax: &Syntax) -> Vec<DefBinding> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() >= 2 {
            if let Some(head) = items[0].as_symbol() {
                match head {
                    "def" | "var" => {
                        if let Some(name) = items[1].as_symbol() {
                            return vec![DefBinding {
                                name: name.to_string(),
                            }];
                        }
                        // Destructuring pattern
                        let mut names = Vec::new();
                        collect_pattern_names(&items[1], &mut names);
                        return names;
                    }
                    "defn" => {
                        if let Some(name) = items[1].as_symbol() {
                            return vec![DefBinding {
                                name: name.to_string(),
                            }];
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    Vec::new()
}

/// Recursively collect leaf symbol names from a destructuring pattern.
fn collect_pattern_names(syntax: &Syntax, out: &mut Vec<DefBinding>) {
    match &syntax.kind {
        SyntaxKind::Symbol(s) if s != "_" => {
            out.push(DefBinding {
                name: s.to_string(),
            });
        }
        SyntaxKind::Array(items) | SyntaxKind::List(items) | SyntaxKind::Struct(items) => {
            for item in items {
                collect_pattern_names(item, out);
            }
        }
        _ => {}
    }
}
