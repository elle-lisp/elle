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

mod pattern;
use pattern::*;

/// Result of compiling a pattern: an optional test expression and the
/// bindings for pattern variables and gensyms.
type PatternResult = Result<(Option<Syntax>, Vec<PatternBinding>), String>;

/// A binding produced by pattern compilation.
///
/// `synthetic` is recorded at the creation site: compiler-generated gensyms
/// are scope-stamped for hygiene, while user pattern variables keep empty
/// scopes so body references resolve. It must be carried as data — deriving
/// it from the NAME (the old `starts_with("__sc")` check) misclassified any
/// user identifier that happened to share the gensym prefix (`__scanner`),
/// stamping it and breaking the user's own references to it.
struct PatternBinding {
    name: String,
    synthetic: bool,
    expr: Syntax,
}

impl PatternBinding {
    /// A user pattern variable: bound by the name the user wrote, scope-free.
    fn user(name: impl Into<String>, expr: Syntax) -> Self {
        PatternBinding {
            name: name.into(),
            synthetic: false,
            expr,
        }
    }

    /// A compiler-generated accessor gensym: scope-stamped on render.
    fn synthetic(name: impl Into<String>, expr: Syntax) -> Self {
        PatternBinding {
            name: name.into(),
            synthetic: true,
            expr,
        }
    }

    /// Render to a (binding-symbol, accessor-expr) pair for `let*`.
    fn into_let_binding(self, span: Span, scope: ScopeId) -> (Syntax, Syntax) {
        let bsym = if self.synthetic {
            make_scoped_symbol(&self.name, span, scope)
        } else {
            Syntax::new(SyntaxKind::Symbol(self.name), span)
        };
        (bsym, self.expr)
    }
}

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
                    // Synthetic (gensym) bindings are scope-stamped for
                    // hygiene; user pattern variables keep empty scopes so
                    // user body references can resolve them — see
                    // PatternBinding. Use let* for sequential evaluation
                    // (list patterns have accessor chains where each binding
                    // depends on the prior).
                    let scoped_bindings = bindings
                        .into_iter()
                        .map(|b| b.into_let_binding(clause.span.clone(), scope))
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
                        .map(|b| b.into_let_binding(clause.span.clone(), scope))
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

/// Make `(let* [b1 e1 b2 e2 ...] body)` for sequential bindings.
fn make_let_star(bindings: Vec<(Syntax, Syntax)>, body: Syntax, span: Span) -> Syntax {
    make_let_form("let*", bindings, body, span)
}

fn make_let_form(
    keyword: &str,
    bindings: Vec<(Syntax, Syntax)>,
    body: Syntax,
    span: Span,
) -> Syntax {
    // Epoch 7 flat bindings: [name1 value1 name2 value2 ...]
    let mut flat_bindings: Vec<Syntax> = Vec::with_capacity(bindings.len() * 2);
    for (bsym, expr) in bindings {
        flat_bindings.push(bsym);
        flat_bindings.push(expr);
    }
    let bindings_node = Syntax::new(SyntaxKind::Array(flat_bindings), span.clone());
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
mod tests;
