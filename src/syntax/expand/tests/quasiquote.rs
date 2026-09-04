//! Quasiquote expansion and keyword handling tests.

use super::*;

#[test]
fn test_quasiquote_simple_list() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // `(a b c)
        let syntax = read_syntax(arena, "`(a b c)", "<test>").unwrap();

        let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
        // Symbols in a quasiquoted list become `SyntaxLiteral(Value::syntax(...))`
        // wrappers (Flatt 2016 §3 — preserves definition-site scopes), so the
        // expansion is `(list #<syntax-literal:...> #<syntax-literal:...> ...)`.
        let result_str = result.to_string();
        assert!(
            result_str.contains("list"),
            "Result should contain 'list': {}",
            result_str
        );
        assert!(
            result_str.contains("syntax-literal"),
            "Quasiquoted symbols should expand to syntax-literal wrappers: {}",
            result_str
        );
    });
}

#[test]
fn test_quasiquote_with_unquote() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // `(a ,x b)
        let syntax = read_syntax(arena, "`(a ,x b)", "<test>").unwrap();

        let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
        // `(a ,x b) expands to `(list <syntax-literal a> x <syntax-literal b>)` —
        // the unquoted `x` appears bare while the other symbols are wrapped.
        let result_str = result.to_string();
        assert!(
            result_str.contains("list"),
            "Result should contain 'list': {}",
            result_str
        );
        // Non-unquoted symbols become SyntaxLiteral for scope preservation
        assert!(
            result_str.contains("syntax-literal"),
            "Quasiquoted symbols should expand to syntax-literal wrappers: {}",
            result_str
        );
        assert!(
            result_str.contains("x"),
            "Unquoted symbol should appear bare: {}",
            result_str
        );
    });
}

#[test]
fn test_quasiquote_with_splicing() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // `(a ,;xs b)
        let syntax = read_syntax(arena, "`(a ,;xs b)", "<test>").unwrap();

        let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
        let result_str = result.to_string();
        assert!(
            result_str.contains("append"),
            "Result should contain 'append': {}",
            result_str
        );
        assert!(
            result_str.contains("list"),
            "Result should contain 'list': {}",
            result_str
        );
        assert!(
            result_str.contains("xs"),
            "Result should contain 'xs': {}",
            result_str
        );
    });
}

#[test]
fn test_quasiquote_non_list() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm, arena) = setup();

        // `x
        let syntax = read_syntax(arena, "`x", "<test>").unwrap();

        let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
        let result_str = result.to_string();
        // A bare quasiquoted symbol expands to a syntax-literal wrapper that
        // carries the original syntax (with its scopes) through the Value
        // round-trip.
        assert!(
            result_str.contains("syntax-literal"),
            "Quasiquoted symbol should expand to a syntax-literal wrapper: {}",
            result_str
        );
        assert!(
            result_str.contains("x"),
            "Result should contain 'x': {}",
            result_str
        );
    });
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
