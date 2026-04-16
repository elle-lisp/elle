//! syntax-case: code-generating pattern matching on syntax objects.
//!
//! `syntax-case` is recognized by the Expander and transformed into a
//! chain of `let`/`if` forms using the syntax predicates. The scrutinee
//! is always bound to a gensym at the outermost level to prevent
//! re-evaluation. No `eval_syntax` calls — this module only produces
//! Syntax, never evaluates anything.
//!
//! Grammar:
//!   (syntax-case \`\<expr\>\`
//!     (\`\<pattern\>\` \`\<body\>\` ...)
//!     (\`\<pattern\>\` when \`\<guard\>\` \`\<body\>\` ...)
//!     ...)
//!
//! Patterns:
//!   _              — wildcard (always matches, no binding)
//!   \`\<symbol\>\`                    — pattern variable (binds scrutinee)
//!   \`\<int/float/string/bool/nil\>\` — literal match
//!   \`\<keyword\>\`                   — literal keyword match (with type guard)
//!   (literal sym)  — literal symbol match (with type guard)
//!   (p1 p2 ... pN) — list pattern (exact length)

use super::Expander;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span, Syntax, SyntaxKind};
use crate::vm::VM;
use std::collections::HashSet;

/// Result of compiling a pattern: an optional test expression and a list of
/// (name, accessor-expr) bindings for pattern variables and gensyms.
type PatternResult = Result<(Option<Syntax>, Vec<(String, Syntax)>), String>;

/// Compile-time counter for generating unique gensym names within
/// a single `syntax-case` expansion. Not globally unique — hygiene
/// is ensured by fresh scopes, not by name uniqueness.
struct GensymCounter(u32);

impl GensymCounter {
    fn new() -> Self {
        GensymCounter(0)
    }

    /// Generate the next name (__sc0, __sc1, ...) and increment.
    fn next(&mut self) -> String {
        let n = self.0;
        self.0 += 1;
        format!("__sc{}", n)
    }
}

/// Pattern kind, parsed from a clause's first element.
enum Pattern {
    Wildcard,
    Variable(String),
    LiteralInt(i64),
    LiteralFloat(f64),
    LiteralString(String),
    LiteralBool(bool),
    LiteralNil,
    LiteralKeyword(String),
    LiteralSymbol(String), // (literal sym)
    List(Vec<Pattern>),
}

impl Expander {
    /// Handle `(syntax-case <expr> clause ...)`.
    pub(super) fn handle_syntax_case(
        &mut self,
        items: &[Syntax],
        span: &Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        // items[0] = `syntax-case`, items[1] = scrutinee, items[2..] = clauses
        if items.len() < 3 {
            return Err(format!(
                "{}: syntax-case requires an expression and at least one clause",
                span
            ));
        }

        let scrutinee_expr = items[1].clone();
        let clauses = &items[2..];

        // Generate a fresh scope for all synthetic bindings.
        let synthetic_scope = self.fresh_scope();

        let mut counter = GensymCounter::new();

        // Bind scrutinee to a gensym at the outermost level.
        let scrut_name = counter.next(); // __sc0
        let scrut_sym = make_scoped_symbol(&scrut_name, span.clone(), synthetic_scope);

        // Generate the clause chain (inner body of the outer let).
        let clause_chain =
            self.compile_clauses(clauses, &scrut_sym, span, synthetic_scope, &mut counter)?;

        // Wrap everything: (let ((__sc0 <scrutinee>)) <clause_chain>)
        let outer_let = make_let(
            vec![(scrut_sym.clone(), scrutinee_expr)],
            clause_chain,
            span.clone(),
        );

        // Recursively expand the result (clause bodies may contain macro calls).
        self.expand(outer_let, symbols, vm)
    }

    /// Compile a sequence of clauses into a nested if/let chain.
    fn compile_clauses(
        &mut self,
        clauses: &[Syntax],
        scrut: &Syntax,
        span: &Span,
        scope: ScopeId,
        counter: &mut GensymCounter,
    ) -> Result<Syntax, String> {
        if clauses.is_empty() {
            // No clauses — unreachable in practice (arity check above requires >= 1)
            return Ok(make_no_match_error(span.clone()));
        }

        let clause = &clauses[0];
        let rest = &clauses[1..];

        // Each clause must be a list.
        let parts = clause
            .as_list_or_tuple()
            .ok_or_else(|| format!("{}: syntax-case clause must be a list", clause.span))?;

        if parts.is_empty() {
            return Err(format!(
                "{}: syntax-case clause must have a pattern and body",
                clause.span
            ));
        }

        let pattern_syn = &parts[0];

        // Check for guard: (pattern when <guard> body...)
        let (guard_opt, body_parts) = if parts.len() >= 3 && parts[1].as_symbol() == Some("when") {
            (Some(&parts[2]), &parts[3..])
        } else {
            (None, &parts[1..])
        };

        if body_parts.is_empty() {
            return Err(format!(
                "{}: syntax-case clause must have a pattern and body",
                clause.span
            ));
        }

        // Parse and validate the pattern.
        let pattern = parse_pattern(pattern_syn)?;

        // Check for duplicate pattern variables.
        let mut seen = HashSet::new();
        collect_pattern_vars(&pattern, &mut seen, pattern_syn)?;

        // Compile the pattern.
        let (test_expr, bindings) = compile_pattern(&pattern, scrut, span, scope, counter)?;

        // The else branch: rest of clauses or no-match error.
        let else_branch = self.compile_clauses(rest, scrut, span, scope, counter)?;

        // The body: multiple body forms wrapped in (begin ...) if more than one.
        let body = if body_parts.len() == 1 {
            body_parts[0].clone()
        } else {
            make_begin(body_parts, &clause.span)
        };

        // Build the result depending on whether there's a test.
        let result = match test_expr {
            None => {
                // Wildcard or variable — unconditional match.
                if bindings.is_empty() {
                    // Wildcard case.
                    if let Some(guard) = guard_opt {
                        // Wildcard with guard: (if guard (let () body) else)
                        let guarded = make_if(
                            guard.clone(),
                            make_let(vec![], body, clause.span.clone()),
                            else_branch,
                            clause.span.clone(),
                        );
                        make_let(vec![], guarded, clause.span.clone())
                    } else {
                        // (let () body)
                        make_let(vec![], body, clause.span.clone())
                    }
                } else {
                    // Variable/list pattern: bind pattern variables.
                    // Gensym bindings (__sc*) are scope-stamped for hygiene.
                    // Pattern variable bindings come from user source and must
                    // have empty scopes so user body references can resolve them.
                    // Use let* for sequential evaluation (list patterns have
                    // accessor chains where each binding depends on the prior).
                    let scoped_bindings = bindings
                        .into_iter()
                        .map(|(name, expr)| {
                            let bsym = if name.starts_with("__sc") {
                                make_scoped_symbol(&name, clause.span.clone(), scope)
                            } else {
                                Syntax::new(SyntaxKind::Symbol(name), clause.span.clone())
                            };
                            (bsym, expr)
                        })
                        .collect();
                    if let Some(guard) = guard_opt {
                        // Build: (let* (...) (if guard body else))
                        let guarded = make_if(
                            guard.clone(),
                            make_let(vec![], body, clause.span.clone()),
                            else_branch,
                            clause.span.clone(),
                        );
                        make_let_star(scoped_bindings, guarded, clause.span.clone())
                    } else {
                        make_let_star(scoped_bindings, body, clause.span.clone())
                    }
                }
            }
            Some(test) => {
                // Has a test expression.
                let then_body = if bindings.is_empty() {
                    let body_let = make_let(vec![], body, clause.span.clone());
                    if let Some(guard) = guard_opt {
                        make_if(
                            guard.clone(),
                            body_let,
                            else_branch.clone(),
                            clause.span.clone(),
                        )
                    } else {
                        body_let
                    }
                } else {
                    let scoped_bindings: Vec<(Syntax, Syntax)> = bindings
                        .into_iter()
                        .map(|(name, expr)| {
                            let bsym = if name.starts_with("__sc") {
                                make_scoped_symbol(&name, clause.span.clone(), scope)
                            } else {
                                Syntax::new(SyntaxKind::Symbol(name), clause.span.clone())
                            };
                            (bsym, expr)
                        })
                        .collect();
                    if let Some(guard) = guard_opt {
                        let guarded = make_if(
                            guard.clone(),
                            make_let(vec![], body, clause.span.clone()),
                            else_branch.clone(),
                            clause.span.clone(),
                        );
                        make_let_star(scoped_bindings, guarded, clause.span.clone())
                    } else {
                        make_let_star(scoped_bindings, body, clause.span.clone())
                    }
                };
                make_if(test, then_body, else_branch, clause.span.clone())
            }
        };

        Ok(result)
    }
}

// =============================================================================
// Pattern parsing
// =============================================================================

/// Parse a pattern Syntax node into a `Pattern`.
fn parse_pattern(syn: &Syntax) -> Result<Pattern, String> {
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
fn collect_pattern_vars(
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
fn compile_pattern(
    pat: &Pattern,
    scrut: &Syntax,
    span: &Span,
    scope: ScopeId,
    counter: &mut GensymCounter,
) -> PatternResult {
    match pat {
        Pattern::Wildcard => Ok((None, vec![])),

        Pattern::Variable(name) => Ok((None, vec![(name.clone(), scrut.clone())])),

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
fn compile_list_pattern(
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
    let mut all_bindings: Vec<(String, Syntax)> = Vec::new();
    let mut current_tail = scrut.clone();

    for (i, sub_pat) in sub_pats.iter().enumerate() {
        // Bind element i to a gensym.
        let elem_name = counter.next();
        let elem_sym = make_scoped_symbol(&elem_name, span.clone(), scope);
        let elem_expr = make_call("syntax-first", vec![current_tail.clone()], span.clone());
        all_bindings.push((elem_name.clone(), elem_expr));

        // Advance the tail (for all but the last element).
        if i + 1 < n {
            let tail_name = counter.next();
            let tail_sym = make_scoped_symbol(&tail_name, span.clone(), scope);
            let tail_expr = make_call("syntax-rest", vec![current_tail.clone()], span.clone());
            all_bindings.push((tail_name.clone(), tail_expr));
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

/// Make a symbol node stamped with `scope`.
fn make_scoped_symbol(name: &str, span: Span, scope: ScopeId) -> Syntax {
    let mut s = Syntax::new(SyntaxKind::Symbol(name.to_string()), span);
    s.add_scope(scope);
    s
}

/// Make `(f arg1 arg2 ...)`.
fn make_call(f: &str, args: Vec<Syntax>, span: Span) -> Syntax {
    let mut items = vec![Syntax::new(SyntaxKind::Symbol(f.to_string()), span.clone())];
    items.extend(args);
    Syntax::new(SyntaxKind::List(items), span)
}

/// Make `(if test then else)`.
fn make_if(test: Syntax, then: Syntax, else_: Syntax, span: Span) -> Syntax {
    Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol("if".to_string()), span.clone()),
            test,
            then,
            else_,
        ]),
        span,
    )
}

/// Make `(let ((b1 e1) (b2 e2) ...) body)`.
/// `bindings` is a vec of (binding-symbol, expr).
fn make_let(bindings: Vec<(Syntax, Syntax)>, body: Syntax, span: Span) -> Syntax {
    make_let_form("let", bindings, body, span)
}

/// Make `(let* ((b1 e1) (b2 e2) ...) body)` for sequential bindings.
fn make_let_star(bindings: Vec<(Syntax, Syntax)>, body: Syntax, span: Span) -> Syntax {
    make_let_form("let*", bindings, body, span)
}

fn make_let_form(
    keyword: &str,
    bindings: Vec<(Syntax, Syntax)>,
    body: Syntax,
    span: Span,
) -> Syntax {
    let binding_list: Vec<Syntax> = bindings
        .into_iter()
        .map(|(bsym, expr)| Syntax::new(SyntaxKind::List(vec![bsym, expr]), span.clone()))
        .collect();
    let bindings_node = Syntax::new(SyntaxKind::List(binding_list), span.clone());
    Syntax::new(
        SyntaxKind::List(vec![
            Syntax::new(SyntaxKind::Symbol(keyword.to_string()), span.clone()),
            bindings_node,
            body,
        ]),
        span,
    )
}

/// Make `(begin form1 form2 ...)` for multiple body forms.
fn make_begin(forms: &[Syntax], span: &Span) -> Syntax {
    let mut items = vec![Syntax::new(
        SyntaxKind::Symbol("begin".to_string()),
        span.clone(),
    )];
    items.extend_from_slice(forms);
    Syntax::new(SyntaxKind::List(items), span.clone())
}

/// Make the no-match runtime error:
/// `(emit 1 {:error :match-error :message "syntax-case: no matching clause"})`.
fn make_no_match_error(span: Span) -> Syntax {
    // Build the struct literal {:error :match-error :message "syntax-case: no matching clause"}
    let struct_node = Syntax::new(
        SyntaxKind::Struct(vec![
            Syntax::new(SyntaxKind::Keyword("error".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Keyword("match-error".to_string()), span.clone()),
            Syntax::new(SyntaxKind::Keyword("message".to_string()), span.clone()),
            Syntax::new(
                SyntaxKind::String("syntax-case: no matching clause".to_string()),
                span.clone(),
            ),
        ]),
        span.clone(),
    );
    make_call(
        "emit",
        vec![
            Syntax::new(SyntaxKind::Int(1), span.clone()), // SIG_ERROR = 1
            struct_node,
        ],
        span,
    )
}

// Behavioral tests (correct return values, pattern matching) are in
// tests/elle/macros.lisp. The Rust tests below cover expansion-time errors
// that cannot be caught from Elle code (they occur before any runtime code runs).

#[cfg(test)]
mod tests {
    use crate::primitives::register_primitives;
    use crate::reader::read_syntax;
    use crate::symbol::SymbolTable;
    use crate::syntax::Expander;
    use crate::vm::VM;

    fn setup() -> (SymbolTable, VM) {
        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        register_primitives(&mut vm, &mut symbols);
        (symbols, vm)
    }

    #[test]
    fn arity_error_no_args() {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("syntax-case requires"));
    }

    #[test]
    fn arity_error_no_clauses() {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("syntax-case requires"));
    }

    #[test]
    fn bad_clause_not_list() {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx 42)", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("clause must be a list"));
    }

    #[test]
    fn duplicate_pattern_variable() {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx ((x x) :body))", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("duplicate pattern variable"));
    }

    #[test]
    fn literal_wrong_arity() {
        let mut expander = Expander::new();
        let (mut symbols, mut vm) = setup();
        expander.load_prelude(&mut symbols, &mut vm).unwrap();

        let syn = read_syntax("(syntax-case stx ((literal) :body))", "<test>").unwrap();
        let result = expander.expand(syn, &mut symbols, &mut vm);
        assert!(result.is_err());
    }
}
