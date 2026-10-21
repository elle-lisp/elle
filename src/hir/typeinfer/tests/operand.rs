// audited: 2026-10-20
// src/hir/AGENTS.md
// docs/impl/graft/operands.md
//! Which operands a graft writes in place of the callee's parameters, and
//! which keep the value they held at the call.

use super::super::infer::var_of;
use super::{compiled, run_booted};
use crate::hir::{BindingArena, Hir, HirKind};
use crate::value::{SymbolId, Value};

/// The binding each `Var` operand of each `%`-intrinsic named `op` reads, by
/// name, in walk order. A read through a capture cell counts as the binding
/// it reads.
fn intrinsic_operand_names(hir: &Hir, arena: &BindingArena, op: &str) -> Vec<SymbolId> {
    fn walk(h: &Hir, arena: &BindingArena, op: &str, out: &mut Vec<SymbolId>) {
        if let HirKind::Intrinsic { op: this, args } = &h.kind {
            if this.name() == op {
                out.extend(args.iter().filter_map(var_of).map(|b| arena.get(b).name));
            }
        }
        h.for_each_child(|c| walk(c, arena, op, out));
    }
    let mut out = Vec::new();
    walk(hir, arena, op, &mut out);
    out
}

/// A loop counter is a written local no closure captures, so the graft writes
/// it in place of the wrapper's parameter, and each op reads the counter
/// itself. The counter-factual is a `let` per use: each op then read the
/// wrapper's own parameter, bound to the counter again on every iteration.
#[test]
fn a_written_local_operand_is_written_in_place() {
    let src = "(defn w [b] (var i 0) (while (< i b) (assign i (+ i 1))) i) (w 5)";
    let (hir, arena) = compiled(src);
    let (i, b) = (SymbolId::of("i"), SymbolId::of("b"));
    assert_eq!(
        intrinsic_operand_names(&hir, &arena, "%lt"),
        vec![i, b],
        "{src}: the comparison reads the counter and the bound"
    );
    assert_eq!(
        intrinsic_operand_names(&hir, &arena, "%add"),
        vec![i],
        "{src}: the increment reads the counter"
    );
}

/// A mutated parameter is written in place on the same terms. Nothing proves
/// it, so the wrapper keeps its guards, and they too read the parameter
/// itself.
#[test]
fn a_written_parameter_operand_is_written_in_place() {
    let src = "(defn w [@n] (while (> n 0) (assign n (- n 1))) n) (w 3)";
    let (hir, arena) = compiled(src);
    let n = SymbolId::of("n");
    assert_eq!(
        intrinsic_operand_names(&hir, &arena, "%gt"),
        vec![n],
        "{src}: the comparison reads the parameter"
    );
    assert_eq!(
        intrinsic_operand_names(&hir, &arena, "%sub"),
        vec![n],
        "{src}: the decrement reads the parameter"
    );
    assert_eq!(run_booted(src), Value::int(0), "{src}");
}

/// A closure that captured the binding may write it from inside the grafted
/// body, so such an operand keeps the value it had at the call. The
/// counter-factual is the body reading the binding after the closure wrote
/// it, which answers 1 in both spellings.
#[test]
fn a_captured_written_operand_keeps_the_value_it_had_at_the_call() {
    let file_scope = "(var i 0) (defn bump [] (assign i (%add i 1))) (defn f [x] (bump) x) (f i)";
    assert_eq!(run_booted(file_scope), Value::int(0), "{file_scope}");
    let local = "(defn g [] (var j 0) (def bump (fn [] (assign j (%add j 1)))) \
                 (defn h [x] (bump) x) (h j)) (g)";
    assert_eq!(run_booted(local), Value::int(0), "{local}");
}

/// An operand after it may write the binding too, and the call evaluated its
/// operands in order. The counter-factual reads k after the second operand
/// wrote it, and answers 11.
#[test]
fn an_operand_a_later_operand_writes_keeps_the_value_it_had_at_the_call() {
    let src = "(defn add2 [x y] (%add x y)) \
               (defn later [] (var k 1) (add2 k (begin (assign k 10) 1))) (later)";
    assert_eq!(run_booted(src), Value::int(2), "{src}");
}
