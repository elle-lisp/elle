//! Unit tests (`super` is the parent impl module).

use super::super::rule::RenameSymbol;
use super::*;
use std::collections::HashMap;

#[test]
fn test_preserves_comments() {
    // Comments are now tokens but no rewrite rule matches them,
    // so comment text survives untouched in the source string.
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

#[test]
fn the_engine_lexes_under_the_lexicon_it_is_given() {
    // `; cons` splices the symbol `cons` under the current lexicon and is a
    // comment under a `;`-commenting one. A rename that fires on the symbol
    // must not fire on the same bytes when they spell a comment.
    let source = "; cons\n(pair a b)\n";
    let mut renames = HashMap::new();
    renames.insert("cons".to_string(), "pair".to_string());
    let rule = RenameSymbol::new("t", renames);
    let rules: Vec<&dyn RewriteRule> = vec![&rule];

    let spliced = collect_edits(
        SourceText::new(source, "t.lisp", Lexicon::current()),
        &rules,
    )
    .unwrap();
    assert_eq!(spliced.len(), 1);
    let commented = collect_edits(
        SourceText::new(source, "t.lisp", Lexicon::divergent()),
        &rules,
    )
    .unwrap();
    assert!(commented.is_empty());
}
