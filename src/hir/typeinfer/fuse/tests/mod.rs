//! Fusion tests, by what the chain is made of.
//!
//! The shared helpers live here; each submodule takes one shape of chain.

use crate::hir::arena::BindingArena;
use crate::hir::expr::{Hir, HirKind};

/// Compile a source form to functionalized HIR against a full stdlib.
///
/// The `Runtime` comes back with the HIR because it owns the display memo, and
/// most of the names these tests assert on — `map`, `*`, `push` — are stdlib
/// exports that no build-time table can spell (docs/impl/symbol.md). Hold it
/// for as long as you want to read a callee's name.
fn compile(src: &str) -> (Hir, BindingArena, crate::runtime::Runtime) {
    let mut rt = crate::runtime::Runtime::new();
    let (hir, arena) = {
        let (_vm, symbols, cctx) = rt.parts();
        crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<test>").expect("compile")
    };
    (hir, arena, rt)
}

/// Names of every call callee (through the ANF/`Var` wrappers) in the tree.
fn callee_names(
    h: &Hir,
    arena: &BindingArena,
    symbols: &crate::symbol::SymbolTable,
    out: &mut Vec<String>,
) {
    if let HirKind::Call { func, .. } = &h.kind {
        if let Some(b) = super::unwrap_callee_binding(func) {
            if let Some(n) = symbols.name(arena.get(b).name) {
                out.push(n.to_string());
            }
        }
    }
    h.for_each_child(|c| callee_names(c, arena, symbols, out));
}

fn callees(h: &Hir, arena: &BindingArena, rt: &mut crate::runtime::Runtime) -> Vec<String> {
    let mut out = Vec::new();
    let (_vm, symbols, _cctx) = rt.parts();
    callee_names(h, arena, symbols, &mut out);
    out
}

/// Count the lambda nodes remaining in the tree — the closure(s) fusion
/// dissolves.
fn count_lambdas(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::Lambda { .. }));
    h.for_each_child(|c| n += count_lambdas(c));
    n
}

/// Count the `if` nodes — a fused `filter` emits one guarded push per
/// predicate stage; a fused `map` emits none.
fn count_ifs(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::If { .. }));
    h.for_each_child(|c| n += count_ifs(c));
    n
}

/// Count the calls to a given op name in the tree.
fn count_callee(
    h: &Hir,
    arena: &BindingArena,
    rt: &mut crate::runtime::Runtime,
    want: &str,
) -> usize {
    callees(h, arena, rt).iter().filter(|n| *n == want).count()
}

/// Count the call-position `%`-intrinsic nodes of a given op in the tree.
///
/// A raw intrinsic is a `HirKind::Intrinsic` node, not a `Call`, so it is
/// invisible to `callees` — this is the discriminator for a spliced kernel.
fn count_intrinsic(h: &Hir, want: &str) -> usize {
    let mut n = usize::from(matches!(&h.kind, HirKind::Intrinsic { op, .. } if op.name() == want));
    h.for_each_child(|c| n += count_intrinsic(c, want));
    n
}

/// Chains whose lambda literal captures an enclosing local — where the capture
/// reaches from, and the composition it declines.
mod capture;
/// `map`, `filter`, their compositions, and the mutable-array cases.
mod collect;
/// The `count` terminal — a guard stage plus a scalar tally.
mod count;
/// The `drop-while` stage — a flag the rejecting element clears, opening the rest
/// of the pipeline.
mod drop;
/// Fold and reduce terminals.
mod fold;
/// The `map-indexed` stage — a transform that reads the walk's induction variable
/// beside the element.
mod indexed;
/// The `mapcat` stage — a fan-out whose element statement carries a walk of its own.
mod mapcat;
/// Chains whose lambda is a named function, same-unit or cross-unit.
mod named;
/// Bodies holding a raw `%`-intrinsic under a `(numeric!)` declaration.
mod numeric;
/// The four short-circuiting search terminals — a guard stage, a scalar answer,
/// and the sentinel the loop condition reads.
mod search;
/// The `take-while` stage — a guard whose rejecting element ends the run.
mod take;
