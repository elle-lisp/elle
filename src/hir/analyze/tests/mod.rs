use super::*;
use crate::syntax::ScopeId;
use crate::syntax::{Span, Syntax, SyntaxKind};

mod basics;
mod bindings;

fn make_span() -> Span {
    Span::new(0, 0, 1, 1)
}

fn make_int(n: i64) -> Syntax {
    Syntax::new(SyntaxKind::Int(n), make_span())
}

fn make_symbol(name: &str) -> Syntax {
    Syntax::new(SyntaxKind::Symbol(name.to_string()), make_span())
}

fn make_list(items: Vec<Syntax>) -> Syntax {
    Syntax::new(SyntaxKind::List(items), make_span())
}

fn make_symbol_scoped(name: &str, scopes: &[u32]) -> Syntax {
    let mut s = make_symbol(name);
    s.scopes = scopes.iter().map(|&n| ScopeId(n)).collect();
    s
}

/// Unwrap a single-expression body that the analyzer may wrap in a Begin.
fn unwrap_single(hir: &Hir) -> &Hir {
    match &hir.kind {
        HirKind::Begin(exprs) if exprs.len() == 1 => &exprs[0],
        _ => hir,
    }
}
