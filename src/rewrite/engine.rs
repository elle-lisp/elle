//! Rewrite engine: lex source, apply rules, produce edits.

#[cfg(test)]
use super::edit::apply_edits;
use super::edit::Edit;
use super::rule::RewriteRule;
use crate::reader::Lexer;

/// Lex source and collect edits from rules without applying them.
pub(crate) fn collect_edits(source: &str, rules: &[&dyn RewriteRule]) -> Result<Vec<Edit>, String> {
    let mut lexer = Lexer::new(source);
    let mut edits = Vec::new();

    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(token)) => {
                for rule in rules {
                    if let Some(edit) = rule.apply(&token) {
                        edits.push(edit);
                        break; // first matching rule wins per token
                    }
                }
            }
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    Ok(edits)
}

/// Rewrite source text by applying rules to each token.
/// Returns (new_source, edits_applied). If no rules match, returns (original_source, empty_vec).
/// Returns Err if lexing fails.
#[cfg(test)]
pub(crate) fn rewrite_source(
    source: &str,
    rules: &[&dyn RewriteRule],
) -> Result<(String, Vec<Edit>), String> {
    let mut edits = collect_edits(source, rules)?;

    if edits.is_empty() {
        return Ok((source.to_string(), Vec::new()));
    }

    let result = apply_edits(source, &mut edits)?;
    Ok((result, edits))
}

#[cfg(test)]
mod tests;
