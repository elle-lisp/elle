//! Rewrite engine: lex source, apply rules, produce edits.

use super::edit::{apply_edits, Edit};
use super::rule::RewriteRule;
use crate::reader::Lexer;

/// Rewrite source text by applying rules to each token.
/// Returns (new_source, edits_applied). If no rules match, returns (original_source, empty_vec).
/// Returns Err if lexing fails.
pub fn rewrite_source(
    source: &str,
    rules: &[&dyn RewriteRule],
) -> Result<(String, Vec<Edit>), String> {
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

    if edits.is_empty() {
        return Ok((source.to_string(), Vec::new()));
    }

    let result = apply_edits(source, &mut edits)?;
    Ok((result, edits))
}

#[cfg(test)]
mod tests {
    use super::super::rule::RenameSymbol;
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_preserves_comments() {
        // Comments are not tokens — lexer skips them.
        // So they must survive untouched.
        let source = "# this is a comment\n(path/join a b)";
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);
        let (result, edits) = rewrite_source(source, &[&rule]).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(result.starts_with("# this is a comment\n"));
        assert!(result.contains("path-join"));
    }

    #[test]
    fn test_no_changes() {
        let source = "(+ 1 2)";
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);
        let (result, edits) = rewrite_source(source, &[&rule]).unwrap();
        assert!(edits.is_empty());
        assert_eq!(result, source);
    }

    #[test]
    fn test_multiple_occurrences() {
        let source = "(path/join (path/join a b) c)";
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);
        let (result, edits) = rewrite_source(source, &[&rule]).unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(result, "(path-join (path-join a b) c)");
    }

    #[test]
    fn test_preserves_strings() {
        let source = "(display \"path/join\") (path/join x)";
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);
        let (result, edits) = rewrite_source(source, &[&rule]).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(result.contains("\"path/join\"")); // string untouched
        assert!(result.contains("(path-join x)"));
    }

    #[test]
    fn test_empty_rules() {
        let source = "(foo bar)";
        let (result, edits) = rewrite_source(source, &[]).unwrap();
        assert!(edits.is_empty());
        assert_eq!(result, source);
    }

    #[test]
    fn test_multibyte_utf8() {
        // Verify byte offsets are correct when source contains multi-byte chars
        let source = "(display \"λ\") (path/join x)";
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);
        let (result, edits) = rewrite_source(source, &[&rule]).unwrap();
        assert_eq!(edits.len(), 1);
        assert!(result.contains("\"λ\"")); // multi-byte string preserved
        assert!(result.contains("(path-join x)"));
    }
}
