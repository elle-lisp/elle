use super::*;

/// Parse a pattern Syntax node into a `Pattern`.
pub(super) fn parse_pattern(syn: &Syntax) -> Result<Pattern, String> {
    match &syn.kind {
        SyntaxKind::Symbol(name) if name == "_" => Ok(Pattern::Wildcard),
        SyntaxKind::Symbol(name) => Ok(Pattern::Variable(name.clone())),
        SyntaxKind::Int(n) => Ok(Pattern::LiteralInt(*n)),
        SyntaxKind::Float(f) => Ok(Pattern::LiteralFloat(*f)),
        SyntaxKind::String(s) => Ok(Pattern::LiteralString(s.clone())),
        SyntaxKind::Bool(b) => Ok(Pattern::LiteralBool(*b)),
        SyntaxKind::Nil => Ok(Pattern::LiteralNil),
        SyntaxKind::Keyword(k) => Ok(Pattern::LiteralKeyword(k.clone())),
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
/// `test_expr` is `None` for unconditional patterns (wildcard, variable).
/// `bindings` maps pattern variable names to accessor expressions.
/// `scrut` is always a symbol (the gensym bound to the scrutinee).
pub(super) fn compile_pattern(
    pat: &Pattern,
    scrut: &Syntax,
    span: &Span,
    scope: ScopeId,
    counter: &mut GensymCounter,
) -> PatternResult {
    match pat {
        Pattern::Wildcard => Ok((None, vec![])),

        Pattern::Variable(name) => Ok((
            None,
            vec![PatternBinding::user(name.clone(), scrut.clone())],
        )),

        Pattern::LiteralInt(n) => {
            // Atoms arrive as plain values in macros; use direct equality.
            // (= scrut N)
            let test = make_call(
                "=",
                vec![
                    scrut.clone(),
                    Syntax::new(SyntaxKind::Int(*n), span.clone()),
                ],
                span.clone(),
            );
            Ok((Some(test), vec![]))
        }

        Pattern::LiteralFloat(f) => {
            // (= scrut F)
            let test = make_call(
                "=",
                vec![
                    scrut.clone(),
                    Syntax::new(SyntaxKind::Float(*f), span.clone()),
                ],
                span.clone(),
            );
            Ok((Some(test), vec![]))
        }

        Pattern::LiteralString(s) => {
            // (= scrut "S")
            let test = make_call(
                "=",
                vec![
                    scrut.clone(),
                    Syntax::new(SyntaxKind::String(s.clone()), span.clone()),
                ],
                span.clone(),
            );
            Ok((Some(test), vec![]))
        }

        Pattern::LiteralBool(b) => {
            // (= scrut B)
            let test = make_call(
                "=",
                vec![
                    scrut.clone(),
                    Syntax::new(SyntaxKind::Bool(*b), span.clone()),
                ],
                span.clone(),
            );
            Ok((Some(test), vec![]))
        }

        Pattern::LiteralNil => {
            // (nil? scrut)
            let test = make_call("nil?", vec![scrut.clone()], span.clone());
            Ok((Some(test), vec![]))
        }

        Pattern::LiteralKeyword(k) => {
            // Keywords arrive as plain Value::keyword in macros.
            // Use direct equality: (= scrut :k)
            let test = make_call(
                "=",
                vec![
                    scrut.clone(),
                    Syntax::new(SyntaxKind::Keyword(k.clone()), span.clone()),
                ],
                span.clone(),
            );
            Ok((Some(test), vec![]))
        }

        Pattern::LiteralSymbol(sym_name) => {
            // (if (syntax-symbol? scrut) (= (syntax-e scrut) 'sym) false)
            let type_check = make_call("syntax-symbol?", vec![scrut.clone()], span.clone());
            let quoted_sym = Syntax::new(
                SyntaxKind::Quote(Box::new(Syntax::new(
                    SyntaxKind::Symbol(sym_name.clone()),
                    span.clone(),
                ))),
                span.clone(),
            );
            let eq_check = make_call(
                "=",
                vec![
                    make_call("syntax-e", vec![scrut.clone()], span.clone()),
                    quoted_sym,
                ],
                span.clone(),
            );
            let test = make_if(
                type_check,
                eq_check,
                Syntax::new(SyntaxKind::Bool(false), span.clone()),
                span.clone(),
            );
            Ok((Some(test), vec![]))
        }

        Pattern::List(sub_pats) => compile_list_pattern(sub_pats, scrut, span, scope, counter),
    }
}

/// Compile a list pattern.
///
/// Returns (length_test, accessor_bindings_plus_sub_bindings).
/// Sub-pattern tests are ANDed into the overall test using nested `if` expressions.
/// Accessor bindings are generated via `syntax-first`/`syntax-rest` chains.
pub(super) fn compile_list_pattern(
    sub_pats: &[Pattern],
    scrut: &Syntax,
    span: &Span,
    scope: ScopeId,
    counter: &mut GensymCounter,
) -> PatternResult {
    let n = sub_pats.len();

    // Primary test: (if (syntax-list? scrut) (= (length (syntax->list scrut)) N) false)
    let type_check = make_call("syntax-list?", vec![scrut.clone()], span.clone());
    let len_check = make_call(
        "=",
        vec![
            make_call(
                "length",
                vec![make_call("syntax->list", vec![scrut.clone()], span.clone())],
                span.clone(),
            ),
            Syntax::new(SyntaxKind::Int(n as i64), span.clone()),
        ],
        span.clone(),
    );
    let mut overall_test: Syntax = make_if(
        type_check,
        len_check,
        Syntax::new(SyntaxKind::Bool(false), span.clone()),
        span.clone(),
    );

    // Generate accessor bindings.
    // For a 3-element list (a b c), the binding sequence is:
    //   (__sc1  (syntax-first __sc0))
    //   (__sc2  (syntax-rest __sc0))
    //   (__sc3  (syntax-first __sc2))
    //   (__sc4  (syntax-rest __sc2))
    //   (__sc5  (syntax-first __sc4))
    // Pattern variables are bound to the element gensyms via sub-pattern bindings.
    let mut all_bindings: Vec<PatternBinding> = Vec::new();
    let mut current_tail = scrut.clone();

    for (i, sub_pat) in sub_pats.iter().enumerate() {
        // Bind element i to a gensym.
        let elem_name = counter.next();
        let elem_sym = make_scoped_symbol(&elem_name, span.clone(), scope);
        let elem_expr = make_call("syntax-first", vec![current_tail.clone()], span.clone());
        all_bindings.push(PatternBinding::synthetic(elem_name, elem_expr));

        // Advance the tail (for all but the last element).
        if i + 1 < n {
            let tail_name = counter.next();
            let tail_sym = make_scoped_symbol(&tail_name, span.clone(), scope);
            let tail_expr = make_call("syntax-rest", vec![current_tail.clone()], span.clone());
            all_bindings.push(PatternBinding::synthetic(tail_name, tail_expr));
            current_tail = tail_sym;
        }

        // Compile the sub-pattern with the element gensym as scrutinee.
        let (sub_test, sub_bindings) = compile_pattern(sub_pat, &elem_sym, span, scope, counter)?;

        // Merge sub-pattern bindings.
        all_bindings.extend(sub_bindings);

        // AND sub-pattern test into overall test.
        if let Some(st) = sub_test {
            overall_test = make_if(
                overall_test,
                st,
                Syntax::new(SyntaxKind::Bool(false), span.clone()),
                span.clone(),
            );
        }
    }

    Ok((Some(overall_test), all_bindings))
}

// =============================================================================
// Syntax construction helpers
// =============================================================================
