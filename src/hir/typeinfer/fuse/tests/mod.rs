// audited: 2026-10-06
//! Fusion tests, by what the chain is made of.
//!
//! The shared helpers live here; each submodule takes one shape of chain.
//!
//! docs/impl/dissolution.md

use crate::hir::arena::BindingArena;
use crate::hir::expr::{Hir, HirKind};

/// Compile a source form to functionalized HIR against a full stdlib.
///
/// The `Runtime` comes back with the HIR because it owns the display memo, and
/// most of the names these tests assert on — `map`, `*`, `push` — are stdlib
/// exports that no build-time table can spell (docs/impl/symbol.md). Hold it
/// for as long as you want to read a callee's name.
fn compile(src: &str) -> (Hir, BindingArena, crate::runtime::Runtime) {
    compile_under(src, crate::config::GraftPolicy::On)
}

/// [`compile`] with the graft off, for a test whose subject is a call the source
/// writes: it stays a call (docs/impl/graft.md).
fn compile_ungrafted(src: &str) -> (Hir, BindingArena, crate::runtime::Runtime) {
    compile_under(src, crate::config::GraftPolicy::Off)
}

fn compile_under(
    src: &str,
    graft: crate::config::GraftPolicy,
) -> (Hir, BindingArena, crate::runtime::Runtime) {
    let mut rt = crate::runtime::Runtime::new();
    rt.compile().set_graft(graft);
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

/// Count the `if` nodes that branch the pipeline — a fused `filter` emits one
/// guarded push per predicate stage; a fused `map` emits none.
///
/// A stage body's stdlib calls are grafted, and a wrapper over an element whose
/// type is not proven keeps its raising guards: `(when (%not (number? b)) (error
/// …))` is an `if` with an `emit` on one side. Those belong to the stage's op,
/// not to the loop's shape, so they are not counted.
fn count_ifs(h: &Hir) -> usize {
    let branches = match &h.kind {
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => !raises(then_branch) && !raises(else_branch),
        _ => false,
    };
    let mut n = usize::from(branches);
    h.for_each_child(|c| n += count_ifs(c));
    n
}

/// Count the `and` nodes. The scaffold emits one only for the loop condition
/// `(and (< i len) more)`, which exactly one early exit claims: the innermost op's,
/// when that op carries one. So the count says where an early exit is read — the
/// loop condition, or a gate stage while the walk stays exhaustive.
fn count_ands(h: &Hir) -> usize {
    let mut n = usize::from(matches!(h.kind, HirKind::And(_)));
    h.for_each_child(|c| n += count_ands(c));
    n
}

/// Does every path through `h` end in an `emit`? The ANF lift names an emit
/// value, so a raise can stand as a `let` init: `(let [t (emit …)] t)`.
fn raises(h: &Hir) -> bool {
    match &h.kind {
        HirKind::Emit { .. } => true,
        HirKind::Begin(xs) => xs.last().is_some_and(raises),
        HirKind::Let { bindings, body } => {
            bindings.iter().any(|(_, init)| raises(init)) || raises(body)
        }
        _ => false,
    }
}

/// Whether the stdlib predicate `name` runs inline: a call to it, or the body the
/// graft puts in its place, which the op that body reaches marks. A fusion test
/// asks whether the predicate runs in the loop, not which spelling it ends in.
fn predicate_inline(
    h: &Hir,
    arena: &BindingArena,
    rt: &mut crate::runtime::Runtime,
    name: &str,
) -> bool {
    const MARKS: [(&str, &str); 3] = [("even?", "%rem"), ("integer?", "type-of"), ("zero?", "%eq")];
    let (_, mark) = MARKS
        .iter()
        .find(|(p, _)| *p == name)
        .expect("a predicate with a known mark");
    count_callee(h, arena, rt, name) > 0
        || count_callee(h, arena, rt, mark) > 0
        || count_intrinsic(h, mark) > 0
}

/// Whether a `fold` runs as a call rather than a fused loop. `fold`'s body
/// closes, so a `fold` fusion declines is grafted, and the call left is its
/// self-recursive driver `core-fold-step`.
fn fold_stays_a_call(cs: &[String]) -> bool {
    cs.iter().any(|n| n == "fold" || n == "core-fold-step")
}

/// Count the loop nodes — a fused chain is one; a declined one is none.
fn count_loops(h: &Hir) -> usize {
    let mut n = usize::from(matches!(
        h.kind,
        HirKind::Loop { .. } | HirKind::While { .. }
    ));
    h.for_each_child(|c| n += count_loops(c));
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

/// How many sites run a stage body's arithmetic or comparison op: a call to the
/// stdlib `wrapper`, or the `%`-op its grafted body reaches (docs/impl/graft.md).
/// A fusion test asks whether the op runs inline, not which of the two spellings
/// it ends in.
///
/// The loop scaffold advances each counter with `(assign i (%add i 1))`, and an
/// `%add` an `assign` stores is not counted, so a `+` assertion cannot pass on the
/// scaffold alone. The scaffold's range test is a call to `<`, or the `%lt` it
/// dissolves to, which does count: a test that needs an inline predicate uses
/// another comparison.
fn count_op(
    h: &Hir,
    arena: &BindingArena,
    rt: &mut crate::runtime::Runtime,
    wrapper: &str,
) -> usize {
    const LOWERED: [(&str, &str); 8] = [
        ("+", "%add"),
        ("-", "%sub"),
        ("*", "%mul"),
        ("<", "%lt"),
        (">", "%gt"),
        ("<=", "%le"),
        (">=", "%ge"),
        ("=", "%eq"),
    ];
    let (_, op) = LOWERED
        .iter()
        .find(|(w, _)| *w == wrapper)
        .expect("an arithmetic or comparison wrapper");
    count_callee(h, arena, rt, wrapper) + count_intrinsic(h, op) - count_advances(h, op)
}

/// The scaffold's counter advances, `(assign c (%add c 1))`, when `op` is `%add`.
fn count_advances(h: &Hir, op: &str) -> usize {
    let advance = match &h.kind {
        HirKind::Assign { target, value } => match &value.kind {
            HirKind::Intrinsic { op: this, args } => {
                this.name() == op
                    && matches!(args.as_slice(),
                        [c, one] if matches!(c.kind, HirKind::Var(b) if b == *target)
                            && matches!(one.kind, HirKind::Int(1)))
            }
            _ => false,
        },
        _ => false,
    };
    let mut n = usize::from(advance);
    h.for_each_child(|c| n += count_advances(c, op));
    n
}

/// `count_op` counts a stage's op and not the scaffold's. The counter-factual:
/// were the counter advance counted, every fused loop would hold a `+` and every
/// `+` assertion would pass on the scaffold alone.
#[test]
fn count_op_skips_the_scaffold() {
    let (hir, arena, mut rt) = compile("(map (fn [x] (* x 2)) [1 2 3])");
    assert!(
        count_intrinsic(&hir, "%add") > 0,
        "the fused loop advances its index with %add"
    );
    assert_eq!(
        count_op(&hir, &arena, &mut rt, "+"),
        0,
        "no stage holds a +"
    );
    assert_eq!(count_op(&hir, &arena, &mut rt, "*"), 1, "the stage's one *");
}

/// Chains whose lambda literal captures an enclosing local — where the capture
/// reaches from, and the composition it declines.
mod capture;
/// `map`, `filter` and their compositions over an immutable base.
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
/// A single op over a mutable `@array` base, and the shapes that decline there.
mod mutable;
/// Chains whose lambda is a named function, same-unit or cross-unit.
mod named;
/// Bodies holding a raw `%`-intrinsic under a `(numeric!)` declaration.
mod numeric;
/// The four short-circuiting search terminals — a guard stage, a scalar answer,
/// and the sentinel the loop condition reads.
mod search;
/// The `take-while` stage — a guard whose rejecting element ends the run.
mod take;
