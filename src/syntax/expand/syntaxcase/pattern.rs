// audited: 2026-11-24
//! syntax-case patterns: parse the pattern of a clause, and compile it to a test and the bindings it makes.
//!
//! docs/macros/syntax.md

use super::*;

/// Parse a pattern Syntax node into a `Pattern`.
pub(super) fn parse_pattern(syn: &Syntax) -> Result<Pattern, String> {
    match &syn.kind {
        SyntaxKind::Symbol(name) if name.as_str() == "_" => Ok(Pattern::Wildcard),
        SyntaxKind::Symbol(name) => Ok(Pattern::Variable(name.to_string())),
        SyntaxKind::Int(n) => Ok(Pattern::LiteralInt(*n)),
        SyntaxKind::Float(f) => Ok(Pattern::LiteralFloat(*f)),
        SyntaxKind::String(s) => Ok(Pattern::LiteralString(s.to_string())),
        SyntaxKind::Bool(b) => Ok(Pattern::LiteralBool(*b)),
        SyntaxKind::Nil => Ok(Pattern::LiteralNil),
        SyntaxKind::Keyword(k) => Ok(Pattern::LiteralKeyword(k.to_string())),
        SyntaxKind::List(items) => {
            // Check for (literal sym)
            if items.first().and_then(|s| s.as_symbol()) == Some("literal") {
                if items.len() != 2 {
                    return Err(format!(
                        "{}: syntax-case: (literal ...) requires exactly one symbol",
                        syn.span
                    ));
                }
                let sym_name = items[1].as_symbol().ok_or_else(|| {
                    format!(
                        "{}: syntax-case: (literal ...) argument must be a symbol",
                        items[1].span
                    )
                })?;
                return Ok(Pattern::LiteralSymbol(sym_name.to_string()));
            }
            // List pattern — recurse on each element.
            let sub_patterns: Result<Vec<Pattern>, String> =
                items.iter().map(parse_pattern).collect();
            Ok(Pattern::List(sub_patterns?))
        }
        _ => Err(format!(
            "{}: syntax-case: unsupported pattern: {}",
            syn.span,
            syn.kind_label()
        )),
    }
}

/// Collect all pattern variable names, erroring on duplicates.
pub(super) fn collect_pattern_vars(
    pat: &Pattern,
    seen: &mut HashSet<String>,
    span_syn: &Syntax,
) -> Result<(), String> {
    match pat {
        Pattern::Variable(name) if !seen.insert(name.clone()) => {
            return Err(format!(
                "{}: syntax-case: duplicate pattern variable '{}'",
                span_syn.span, name
            ));
        }
        Pattern::Variable(_) => {}
        Pattern::List(sub_pats) => {
            for sp in sub_pats {
                collect_pattern_vars(sp, seen, span_syn)?;
            }
        }
        _ => {}
    }
    Ok(())
}

// =============================================================================
// Pattern compilation
// =============================================================================

/// Compile a pattern to (test_expr_or_None, bindings).
///
/// `scrut` is an expression that answers the node under test: the bound
/// scrutinee, or an accessor into it for an element of a list pattern. A test
/// reads an element through its accessor, because the clause binds its pattern
/// variables only once the whole test holds. `test_expr` is `None` for an
/// unconditional pattern (wildcard, variable), and `bindings` maps each
/// pattern variable to its accessor.
pub(super) fn compile_pattern(
    arena: &SyntaxArena,
    pat: &Pattern,
    scrut: &Syntax,
    span: &Span,
) -> PatternResult {
    // A literal reads the node as data, so an atom argument and an atom inside
    // a list compare by value, and a plain value passes through unchanged.
    let datum = || make_call(arena, "syntax/->datum", vec![*scrut], *span);
    let equals = |lit: Syntax| make_call(arena, "=", vec![datum(), lit], *span);

    match pat {
        Pattern::Wildcard => Ok((None, vec![])),

        Pattern::Variable(name) => Ok((None, vec![PatternBinding::new(name.clone(), *scrut)])),

        Pattern::LiteralInt(n) => Ok((
            Some(equals(Syntax::new(SyntaxKind::Int(*n), *span))),
            vec![],
        )),

        Pattern::LiteralFloat(f) => Ok((
            Some(equals(Syntax::new(SyntaxKind::Float(*f), *span))),
            vec![],
        )),

        Pattern::LiteralString(s) => Ok((Some(equals(Syntax::string(arena, s, *span))), vec![])),

        Pattern::LiteralBool(b) => Ok((
            Some(equals(Syntax::new(SyntaxKind::Bool(*b), *span))),
            vec![],
        )),

        Pattern::LiteralNil => Ok((Some(make_call(arena, "nil?", vec![datum()], *span)), vec![])),

        Pattern::LiteralKeyword(k) => Ok((Some(equals(Syntax::keyword(arena, k, *span))), vec![])),

        Pattern::LiteralSymbol(sym_name) => {
            // (if (syntax/symbol? scrut) (= (syntax/e scrut) 'sym) false)
            let type_check = make_call(arena, "syntax/symbol?", vec![*scrut], *span);
            let quoted_sym = Syntax::quote(arena, Syntax::symbol(arena, sym_name, *span), *span);
            let eq_check = make_call(
                arena,
                "=",
                vec![
                    make_call(arena, "syntax/e", vec![*scrut], *span),
                    quoted_sym,
                ],
                *span,
            );
            let test = make_if(
                arena,
                type_check,
                eq_check,
                Syntax::new(SyntaxKind::Bool(false), *span),
                *span,
            );
            Ok((Some(test), vec![]))
        }

        Pattern::List(sub_pats) => compile_list_pattern(arena, sub_pats, scrut, span),
    }
}

/// Compile a list pattern.
///
/// The test checks the node is a list of the pattern's length, then each
/// element's own test, in order, so an element's accessor runs only on a list
/// long enough to hold it. Element `i` is `(get (syntax/->list scrut) i)`.
pub(super) fn compile_list_pattern(
    arena: &SyntaxArena,
    sub_pats: &[Pattern],
    scrut: &Syntax,
    span: &Span,
) -> PatternResult {
    let and_then = |test: Syntax, next: Syntax| {
        make_if(
            arena,
            test,
            next,
            Syntax::new(SyntaxKind::Bool(false), *span),
            *span,
        )
    };
    let elements = || make_call(arena, "syntax/->list", vec![*scrut], *span);

    // (if (syntax/list? scrut) (= (length (syntax/->list scrut)) N) false)
    let type_check = make_call(arena, "syntax/list?", vec![*scrut], *span);
    let len_check = make_call(
        arena,
        "=",
        vec![
            make_call(arena, "length", vec![elements()], *span),
            Syntax::new(SyntaxKind::Int(sub_pats.len() as i64), *span),
        ],
        *span,
    );
    let mut overall_test = and_then(type_check, len_check);
    let mut all_bindings: Vec<PatternBinding> = Vec::new();

    for (i, sub_pat) in sub_pats.iter().enumerate() {
        let index = Syntax::new(SyntaxKind::Int(i as i64), *span);
        let element = make_call(arena, "get", vec![elements(), index], *span);
        let (sub_test, sub_bindings) = compile_pattern(arena, sub_pat, &element, span)?;
        all_bindings.extend(sub_bindings);
        if let Some(st) = sub_test {
            overall_test = and_then(overall_test, st);
        }
    }

    Ok((Some(overall_test), all_bindings))
}
