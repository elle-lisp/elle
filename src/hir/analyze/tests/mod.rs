use super::*;
use crate::syntax::ScopeId;
use crate::syntax::{thread_arena, SeqCtor, Span, Syntax, SyntaxArena, SyntaxKind};

mod basics;
mod bindings;

fn make_span() -> Span {
    Span::new(0, 0, 1, 1)
}

fn make_int(n: i64) -> Syntax {
    Syntax::new(SyntaxKind::Int(n), make_span())
}

fn make_symbol(name: &str) -> Syntax {
    Syntax::symbol(&thread_arena(), name, make_span())
}

fn make_list(items: Vec<Syntax>) -> Syntax {
    Syntax::list(&thread_arena(), &items, make_span())
}

fn make_array(items: Vec<Syntax>) -> Syntax {
    Syntax::array(&thread_arena(), &items, make_span())
}

/// A node of sequence kind `make` over `items` — the collection literals the
/// splice tests build by name.
fn make_seq(make: SeqCtor, items: Vec<Syntax>) -> Syntax {
    Syntax::new(make(thread_arena().nodes(&items)), make_span())
}

/// A `;expr` splice node wrapping `inner`.
fn make_splice(inner: Syntax) -> Syntax {
    Syntax::new(SyntaxKind::Splice(thread_arena().node(inner)), make_span())
}

fn make_symbol_scoped(name: &str, scopes: &[u32]) -> Syntax {
    let arena: SyntaxArena = thread_arena();
    let scopes: Vec<ScopeId> = scopes.iter().map(|&n| ScopeId(n)).collect();
    Syntax::symbol_scoped(&arena, name, make_span(), &scopes)
}

/// Unwrap a single-expression body that the analyzer may wrap in a Begin.
fn unwrap_single(hir: &Hir) -> &Hir {
    match &hir.kind {
        HirKind::Begin(exprs) if exprs.len() == 1 => &exprs[0],
        _ => hir,
    }
}
