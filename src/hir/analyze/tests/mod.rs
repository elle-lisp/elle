// audited: 2026-11-10
//! The analyzer's unit tests, and the syntax builders and analysis helpers they share.
//!
//! docs/impl/hir.md

use super::*;
use crate::syntax::ScopeId;
use crate::syntax::{thread_arena, SeqCtor, Span, Syntax, SyntaxArena, SyntaxKind};

mod basics;
mod bindings;
mod body;
mod environment;
mod fixpoint;
mod merged;
mod messages;
mod nested;
mod prune;
mod qualified;
mod selfedge;
mod writer;

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

/// `source` analyzed as a file, as the linter and `compile/analyze` see it.
fn analyze_file(source: &str) -> crate::pipeline::AnalyzeResult {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    crate::pipeline::analyze_file(source, symbols, vm, cctx, "<analyze-test>")
        .unwrap_or_else(|e| panic!("{source} did not analyze: {e}"))
}

/// Every node of `hir`, outermost first.
fn nodes(hir: &Hir) -> Vec<&Hir> {
    fn collect<'a>(hir: &'a Hir, out: &mut Vec<&'a Hir>) {
        out.push(hir);
        hir.for_each_child(|child| collect(child, out));
    }
    let mut out = Vec::new();
    collect(hir, &mut out);
    out
}

/// The inferred signal of the lambda a binding spelled `name` takes, wherever
/// in `result` a `Define`, a `Let` or a `Letrec` binds it.
fn signal_of(result: &crate::pipeline::AnalyzeResult, name: &str) -> Signal {
    let named = |b: Binding| result.arena.get(b).name == SymbolId::of(name);
    let lambda = |value: &Hir| match &value.kind {
        HirKind::Lambda(lambda) => Some(lambda.inferred_signals),
        _ => None,
    };
    nodes(&result.hir)
        .into_iter()
        .find_map(|node| match &node.kind {
            HirKind::Define { binding, value } if named(*binding) => lambda(value),
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => bindings
                .iter()
                .find_map(|(b, value)| if named(*b) { lambda(value) } else { None }),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no lambda is bound to {name}"))
}

/// Unwrap a single-expression body that the analyzer may wrap in a Begin.
fn unwrap_single(hir: &Hir) -> &Hir {
    match &hir.kind {
        HirKind::Begin(exprs) if exprs.len() == 1 => &exprs[0],
        _ => hir,
    }
}
