//! Rewrite engine: lex source, apply rules, produce edits.

#[cfg(test)]
use super::edit::apply_edits;
use super::edit::Edit;
use super::rule::RewriteRule;
use super::text::SourceText;
#[cfg(test)]
use crate::epoch::rules::Lexicon;

/// Lex source and collect edits from rules without applying them.
pub(crate) fn collect_edits(
    source: SourceText<'_>,
    rules: &[&dyn RewriteRule],
) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();

    for token in source.tokens()? {
        for rule in rules {
            if let Some(edit) = rule.apply(&token) {
                edits.push(edit);
                break; // first matching rule wins per token
            }
        }
    }

    Ok(edits)
}

/// Rewrite source text by applying rules to each token, under the current
/// epoch's lexicon.
/// Returns (new_source, edits_applied). If no rules match, returns (original_source, empty_vec).
/// Returns Err if lexing fails.
#[cfg(test)]
pub(crate) fn rewrite_source(
    source: &str,
    rules: &[&dyn RewriteRule],
) -> Result<(String, Vec<Edit>), String> {
    let text = SourceText::new(source, "<rewrite>", Lexicon::current());
    let mut edits = collect_edits(text, rules)?;

    if edits.is_empty() {
        return Ok((source.to_string(), Vec::new()));
    }

    let result = apply_edits(source, &mut edits)?;
    Ok((result, edits))
}

#[cfg(test)]
mod tests;
