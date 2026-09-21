// audited: 2026-09-21
//! Library migration rules driving the edit engine: rename, replace and
//! report rules supplied as data, applied to a fixpoint.
//!
//! docs/analysis/portrait.md

use std::collections::HashMap;

use super::edit::{apply_edits, Edit};
use super::engine::collect_edits;
use super::rule::{RenameSymbol, RewriteRule};
use super::run::edits::collect_replace_edits;
use super::text::SourceText;
use crate::epoch::detect_epoch_in_source;
use crate::epoch::rules::{Lexicon, CURRENT_EPOCH};
use crate::reader::Token;

/// The rules one application carries, already validated by the caller.
#[derive(Default)]
pub(crate) struct LibraryRules {
    /// Exact symbol spellings to rename.
    pub renames: HashMap<String, String>,
    /// (head symbol, arity, template with `$N` placeholders).
    pub replaces: Vec<(String, usize, String)>,
    /// (symbol, message) — occurrences are reported, never edited.
    pub reports: Vec<(String, String)>,
}

/// One reported occurrence of a symbol a report rule names.
#[derive(Debug)]
pub(crate) struct Occurrence {
    pub name: String,
    pub message: String,
    pub line: usize,
}

/// What an application produced: the rewritten text, how many edits it
/// took, and the report occurrences from the original text.
#[derive(Debug)]
pub(crate) struct Applied {
    pub source: String,
    pub count: usize,
    pub reports: Vec<Occurrence>,
}

/// A template naming its own head never converges; stop loudly instead.
const MAX_PASSES: usize = 10;

/// Apply library migration rules to `source`, reapplying until the text
/// stops moving — template interpolation copies argument bytes verbatim,
/// so a rename inside a replaced call's arguments needs a later pass.
/// `name` is for error messages only.
pub(crate) fn apply_library_rules(
    source: &str,
    name: &str,
    rules: &LibraryRules,
) -> Result<Applied, String> {
    let reports = collect_reports(source, name, &rules.reports)?;
    let mut text = source.to_string();
    let mut count = 0;
    for _ in 0..MAX_PASSES {
        let mut edits = one_pass(&text, name, rules)?;
        if edits.is_empty() {
            return Ok(Applied {
                source: text,
                count,
                reports,
            });
        }
        count += edits.len();
        text = apply_edits(&text, &mut edits)?;
    }
    Err(format!(
        "{}: migration rules did not converge after {} passes",
        name, MAX_PASSES
    ))
}

/// The text under its own epoch's lexicon, like every rewrite pass
/// (docs/impl/lexicon.md).
fn source_text<'a>(text: &'a str, name: &'a str) -> Result<SourceText<'a>, String> {
    let epoch = detect_epoch_in_source(text)?
        .map(|info| info.epoch)
        .unwrap_or(CURRENT_EPOCH);
    Ok(SourceText::new(text, name, Lexicon::for_epoch(epoch)))
}

/// One round of edits: structural replaces win over renames that fall
/// inside their spans, exactly as `elle rewrite` merges them.
fn one_pass(text: &str, name: &str, rules: &LibraryRules) -> Result<Vec<Edit>, String> {
    let st = source_text(text, name)?;
    let replaces: Vec<(&str, usize, &str)> = rules
        .replaces
        .iter()
        .map(|(s, a, t)| (s.as_str(), *a, t.as_str()))
        .collect();
    let replace_edits = if replaces.is_empty() {
        Vec::new()
    } else {
        collect_replace_edits(st, &replaces)?
    };
    let mut edits = if rules.renames.is_empty() {
        Vec::new()
    } else {
        let rule = RenameSymbol::new("library-migration", rules.renames.clone());
        let dyn_rules: Vec<&dyn RewriteRule> = vec![&rule];
        collect_edits(st, &dyn_rules)?
    };
    edits.retain(|e| {
        !replace_edits.iter().any(|re| {
            e.byte_offset >= re.byte_offset
                && e.byte_offset + e.byte_len <= re.byte_offset + re.byte_len
        })
    });
    edits.extend(replace_edits);
    Ok(edits)
}

/// Every occurrence of a reported symbol in the original text, in order.
fn collect_reports(
    text: &str,
    name: &str,
    wanted: &[(String, String)],
) -> Result<Vec<Occurrence>, String> {
    if wanted.is_empty() {
        return Ok(Vec::new());
    }
    let st = source_text(text, name)?;
    let mut out = Vec::new();
    for token in st.tokens()? {
        if let Token::Symbol(sym) = &token.token {
            for (n, msg) in wanted {
                if n == sym {
                    out.push(Occurrence {
                        name: n.clone(),
                        message: msg.clone(),
                        line: token.loc.line,
                    });
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
