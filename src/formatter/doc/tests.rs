//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_empty_doc() {
    let doc = Doc::empty();
    assert!(matches!(doc, Doc::Empty));
}

#[test]
fn test_text_doc() {
    let doc = Doc::text("hello");
    assert!(matches!(doc, Doc::Text(s) if s == "hello"));
}

#[test]
fn test_concat_flatten_single() {
    let doc = Doc::concat(vec![Doc::text("x")]);
    // Single-element concat is unwrapped
    assert!(matches!(doc, Doc::Text(s) if s == "x"));
}

#[test]
fn test_concat_empty() {
    let doc = Doc::concat(vec![]);
    assert!(matches!(doc, Doc::Empty));
}

#[test]
fn test_nest_empty() {
    let doc = Doc::empty().nest(2);
    // Nesting empty is still empty
    assert!(matches!(doc, Doc::Empty));
}

#[test]
fn test_group_empty() {
    let doc = Doc::empty().group();
    // Grouping empty is still empty
    assert!(matches!(doc, Doc::Empty));
}

#[test]
fn test_intersperse_empty() {
    let doc = Doc::intersperse(vec![]);
    assert!(matches!(doc, Doc::Empty));
}

#[test]
fn test_intersperse_single() {
    let doc = Doc::intersperse(vec![Doc::text("x")]);
    assert!(matches!(doc, Doc::Text(s) if s == "x"));
}

#[test]
fn test_builder_chaining() {
    let doc = Doc::concat([Doc::text("hello"), Doc::text(" "), Doc::text("world")]).group();
    assert!(matches!(doc, Doc::Group(_)));
}
