//! Quasiquote expansion and keyword handling tests.

use super::*;

#[test]
fn test_quasiquote_simple_list() {
    crate::value::arena::with_test_region(|| {
        let (mut expander, mut symbols, mut vm) = setup();
        let span = Span::new(0, 10, 1, 1);

        // `(a b c)
        let items = vec![
            Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Symbol("c".to_string()), span.clone()),
        ];
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
            span.clone(),
        );

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
        let (mut expander, mut symbols, mut vm) = setup();
        let span = Span::new(0, 10, 1, 1);

        // `(a ,x b)
        let items = vec![
            Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::Unquote(Box::new(Syntax::new(
                    SyntaxKind::Symbol("x".to_string()),
                    span.clone(),
                ))),
                span.clone(),
            ),
            Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
        ];
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
            span.clone(),
        );

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
        let (mut expander, mut symbols, mut vm) = setup();
        let span = Span::new(0, 10, 1, 1);

        // `(a ,;xs b)
        let items = vec![
            Syntax::new(SyntaxKind::Symbol("a".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::UnquoteSplicing(Box::new(Syntax::new(
                    SyntaxKind::Symbol("xs".to_string()),
                    span.clone(),
                ))),
                span.clone(),
            ),
            Syntax::new(SyntaxKind::Symbol("b".to_string()), span.clone()),
        ];
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(SyntaxKind::List(items), span.clone()))),
            span.clone(),
        );

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
        let (mut expander, mut symbols, mut vm) = setup();
        let span = Span::new(0, 5, 1, 1);

        // `x
        let syntax = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(Syntax::new(
                SyntaxKind::Symbol("x".to_string()),
                span.clone(),
            ))),
            span.clone(),
        );

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
        let (mut expander, mut symbols, mut vm) = setup();
        let span = Span::new(0, 5, 1, 1);

        // :keyword should remain a keyword, not be treated as qualified
        let syntax = Syntax::new(SyntaxKind::Keyword("foo".to_string()), span);
        let result = expander.expand(syntax, &mut symbols, &mut vm).unwrap();
        // Keywords are stored without the leading colon in SyntaxKind::Keyword
        assert!(matches!(result.kind, SyntaxKind::Keyword(ref s) if s == "foo"));
    });
}
