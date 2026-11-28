// audited: 2026-11-24
//! What a quasiquote lowers to outside a transformer, and keyword handling.
//!
//! src/syntax/expand/AGENTS.md
//!
//! These expansions run outside any transformer's compile, so a template
//! symbol is quoted data. The language suite pins the transformer side
//! (`tests/lang/quoting.lisp`, `tests/lang/hygiene.lisp`).

use super::*;

/// The expansion of `text`, printed.
fn expanded(text: &str) -> String {
    let mut printed = String::new();
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();
        let syntax = read_syntax(arena, text, "<test>").unwrap();
        printed = expander
            .expand(syntax, &mut symbols, &mut vm)
            .unwrap()
            .to_string();
    });
    printed
}

#[test]
fn a_template_symbol_outside_a_transformer_is_quoted() {
    let out = expanded("`(a b c)");
    assert!(out.contains("list"), "{out}");
    assert!(!out.contains("syntax-literal"), "{out}");
    assert!(expanded("`x") == "'x", "a bare symbol is its quote");
}

#[test]
fn an_unquote_is_the_expression_itself() {
    let out = expanded("`(a ,x b)");
    assert!(!out.contains("syntax-literal"), "{out}");
    assert!(out.contains(" x "), "{out}");
}

#[test]
fn an_unquote_splice_spreads_into_the_constructor_call() {
    // A spread takes a list, an array or an @array, where `append` took a
    // list alone.
    for text in ["`(a ,;xs b)", "`[a ,;xs b]"] {
        let out = expanded(text);
        assert!(out.contains(";xs"), "{text} expanded to {out}");
        assert!(!out.contains("append"), "{text} expanded to {out}");
    }
}

#[test]
fn a_spelled_quasiquote_lowers_as_its_prefix_does() {
    assert_eq!(
        expanded("(quasiquote (a (unquote x)))"),
        expanded("`(a ,x)")
    );
}

#[test]
fn test_keyword_not_qualified() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // :keyword should remain a keyword, not be treated as qualified
        let syntax = read_syntax(arena, ":foo", "<test>").unwrap();
        let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
        // Keywords are stored without the leading colon in SyntaxKind::Keyword
        assert!(matches!(result.kind, SyntaxKind::Keyword(s) if s.as_str() == "foo"));
    });
}
