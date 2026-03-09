//! Rewrite rule trait and built-in rules.

use super::edit::Edit;
use crate::reader::{Token, TokenWithLoc};
use std::collections::HashMap;

/// A rewrite rule that examines a token and optionally produces an edit.
pub(crate) trait RewriteRule {
    /// Human-readable rule name.
    fn name(&self) -> &str;

    /// Examine a token and optionally produce a source edit.
    fn apply(&self, token: &TokenWithLoc) -> Option<Edit>;
}

/// Rename symbols by exact match. Data-driven from a HashMap.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct RenameSymbol {
    rule_name: String,
    renames: HashMap<String, String>,
}

impl RenameSymbol {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(name: impl Into<String>, renames: HashMap<String, String>) -> Self {
        RenameSymbol {
            rule_name: name.into(),
            renames,
        }
    }
}

impl RewriteRule for RenameSymbol {
    fn name(&self) -> &str {
        &self.rule_name
    }

    fn apply(&self, token: &TokenWithLoc) -> Option<Edit> {
        if let Token::Symbol(name) = &token.token {
            if let Some(new_name) = self.renames.get(*name) {
                return Some(Edit {
                    byte_offset: token.byte_offset,
                    byte_len: token.len,
                    replacement: new_name.clone(),
                });
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::Lexer;

    fn lex_tokens(source: &str) -> Vec<crate::reader::TokenWithLoc<'_>> {
        let mut lexer = Lexer::new(source);
        let mut tokens = Vec::new();
        while let Ok(Some(tok)) = lexer.next_token_with_loc() {
            tokens.push(tok);
        }
        tokens
    }

    #[test]
    fn test_rename_matches() {
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);

        let tokens = lex_tokens("(path/join a b)");
        let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].replacement, "path-join");
    }

    #[test]
    fn test_rename_no_match() {
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);

        let tokens = lex_tokens("(+ 1 2)");
        let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
        assert_eq!(edits.len(), 0);
    }

    #[test]
    fn test_rename_ignores_keywords() {
        let mut renames = HashMap::new();
        renames.insert("foo".to_string(), "bar".to_string());
        let rule = RenameSymbol::new("test", renames);

        let tokens = lex_tokens(":foo");
        let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
        assert_eq!(edits.len(), 0);
    }

    #[test]
    fn test_rename_ignores_strings() {
        let mut renames = HashMap::new();
        renames.insert("path/join".to_string(), "path-join".to_string());
        let rule = RenameSymbol::new("test", renames);

        let tokens = lex_tokens("\"path/join\"");
        let edits: Vec<Edit> = tokens.iter().filter_map(|t| rule.apply(t)).collect();
        assert_eq!(edits.len(), 0);
    }
}
