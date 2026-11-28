// audited: 2026-11-24
//! syntax-case: code-generating pattern matching on syntax objects.
//!
//! docs/macros/syntax.md
//!
//! `syntax-case` is recognized by the Expander and transformed into a
//! chain of `let`/`if` forms using the syntax predicates. The scrutinee
//! is bound once, at the outermost level, to prevent
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
//!   \`\<int/float/string/bool/nil\>\` — literal match on the node's datum
//!   \`\<keyword\>\`                   — literal keyword match on the node's datum
//!   (literal sym)  — literal symbol match (with type guard)
//!   (p1 p2 ... pN) — list pattern (exact length)

use super::Expander;
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};
use crate::vm::VM;
use std::collections::HashSet;

mod pattern;
use pattern::*;

/// Result of compiling a pattern: an optional test expression and the
/// bindings for its pattern variables.
type PatternResult = Result<(Option<Syntax>, Vec<PatternBinding>), String>;

/// A pattern variable and the accessor expression that reads its node.
///
/// The variable keeps the name the user wrote with no scopes, so the clause
/// body's references resolve to it. Only the scrutinee's own name carries the
/// expansion's fresh scope, so a user variable that shares its spelling
/// cannot capture it.
struct PatternBinding {
    name: String,
    expr: Syntax,
}

impl PatternBinding {
    fn new(name: impl Into<String>, expr: Syntax) -> Self {
        PatternBinding {
            name: name.into(),
            expr,
        }
    }

    /// Render to a (binding-symbol, accessor-expr) pair for `let`.
    fn into_let_binding(self, arena: &SyntaxArena, span: Span) -> (Syntax, Syntax) {
        (Syntax::symbol(arena, &self.name, span), self.expr)
    }
}

/// The name the scrutinee is bound to, under the expansion's fresh scope.
const SCRUTINEE: &str = "__sc";

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

        let arena = self.arena();
        let scrutinee_expr = items[1];
        let clauses = &items[2..];

        // Bind the scrutinee once, at the outermost level, under a fresh scope.
        let scope = self.fresh_scope();
        let scrut_sym = Syntax::symbol_scoped(&arena, SCRUTINEE, *span, &[scope]);

        // Generate the clause chain (inner body of the outer let).
        let clause_chain = self.compile_clauses(clauses, &scrut_sym, span)?;

        // Wrap everything: (let [__sc <scrutinee>] <clause_chain>)
        let outer_let = make_let(
            &arena,
            vec![(scrut_sym, scrutinee_expr)],
            clause_chain,
            *span,
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
    ) -> Result<Syntax, String> {
        let arena = self.arena();
        if clauses.is_empty() {
            // Past the last clause: the expanded code signals :match-error here.
            return Ok(make_no_match_error(&arena, *span));
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
        let (test_expr, bindings) = compile_pattern(&arena, &pattern, scrut, span)?;

        // The else branch: rest of clauses or no-match error.
        let else_branch = self.compile_clauses(rest, scrut, span)?;

        // The body: multiple body forms wrapped in (begin ...) if more than one.
        let body = if body_parts.len() == 1 {
            body_parts[0]
        } else {
            make_begin(&arena, body_parts, &clause.span)
        };

        // (let [vars…] body), or (let [vars…] (if guard (let [] body) else))
        // for a clause with a guard, which reads the pattern's variables.
        let bindings = bindings
            .into_iter()
            .map(|b| b.into_let_binding(&arena, clause.span))
            .collect();
        let inner = match guard_opt {
            Some(guard) => make_if(
                &arena,
                *guard,
                make_let(&arena, vec![], body, clause.span),
                else_branch,
                clause.span,
            ),
            None => body,
        };
        let matched = make_let(&arena, bindings, inner, clause.span);

        Ok(match test_expr {
            Some(test) => make_if(&arena, test, matched, else_branch, clause.span),
            None => matched,
        })
    }
}

// =============================================================================
// Syntax builders
// =============================================================================

/// Make `(f arg1 arg2 ...)`.
fn make_call(arena: &SyntaxArena, f: &str, args: Vec<Syntax>, span: Span) -> Syntax {
    let mut items = vec![Syntax::symbol(arena, f, span)];
    items.extend(args);
    Syntax::list(arena, &items, span)
}

/// Make `(if test then else)`.
fn make_if(arena: &SyntaxArena, test: Syntax, then: Syntax, else_: Syntax, span: Span) -> Syntax {
    Syntax::list(
        arena,
        &[Syntax::symbol(arena, "if", span), test, then, else_],
        span,
    )
}

/// Make `(let [b1 e1 b2 e2 ...] body)`.
/// `bindings` is a vec of (binding-symbol, expr).
fn make_let(
    arena: &SyntaxArena,
    bindings: Vec<(Syntax, Syntax)>,
    body: Syntax,
    span: Span,
) -> Syntax {
    let flat_bindings: Vec<Syntax> = bindings
        .into_iter()
        .flat_map(|(bsym, expr)| [bsym, expr])
        .collect();
    let bindings_node = Syntax::array(arena, &flat_bindings, span);
    Syntax::list(
        arena,
        &[Syntax::symbol(arena, "let", span), bindings_node, body],
        span,
    )
}

/// Make `(begin form1 form2 ...)` for multiple body forms.
fn make_begin(arena: &SyntaxArena, forms: &[Syntax], span: &Span) -> Syntax {
    let mut items = vec![Syntax::symbol(arena, "begin", *span)];
    items.extend_from_slice(forms);
    Syntax::list(arena, &items, *span)
}

/// Make the no-match runtime error:
/// `(emit 1 {:error :match-error :message "syntax-case: no matching clause"})`.
fn make_no_match_error(arena: &SyntaxArena, span: Span) -> Syntax {
    // Build the struct literal {:error :match-error :message "syntax-case: no matching clause"}
    let struct_node = Syntax::new(
        SyntaxKind::Struct(arena.nodes(&[
            Syntax::keyword(arena, "error", span),
            Syntax::keyword(arena, "match-error", span),
            Syntax::keyword(arena, "message", span),
            Syntax::string(arena, "syntax-case: no matching clause", span),
        ])),
        span,
    );
    make_call(
        arena,
        "emit",
        vec![
            Syntax::new(SyntaxKind::Int(1), span), // SIG_ERROR = 1
            struct_node,
        ],
        span,
    )
}

// Behavioral tests (correct return values, pattern matching) are in
// tests/lang/syntax-case.lisp. The Rust tests below cover expansion-time errors
// that cannot be caught from Elle code (they occur before any runtime code runs).

#[cfg(test)]
mod tests;
