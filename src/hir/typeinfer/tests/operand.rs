// audited: 2026-11-20
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

/// A loop counter is a written local no closure writes, so the graft writes
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

/// A loop counter a closure captures and only reads is still a local no
/// closure writes, so the graft writes it in place, and each op reads the
/// counter's cell itself. The counter-factual is the `let` per use the capture
/// used to cost: the capture then stood in for a write, and each op read a
/// fresh local instead of the cell.
#[test]
fn a_captured_local_no_closure_writes_is_written_in_place() {
    let src = "(defn w [b] (var i 0) (def show (fn [] i)) \
               (while (< i b) (assign i (+ i 1))) (show)) (w 5)";
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
    assert_eq!(run_booted(src), Value::int(5), "{src}");
}

/// A closure that writes the binding may do so from inside the grafted body,
/// so such an operand keeps the value it had at the call. The counter-factual
/// is the body reading the binding after the closure wrote it, which answers
/// 1 in both spellings.
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

/// The function that owns `lo` writes it while the lambda's code waits in a
/// call: the call resumes the fiber where the owner is parked. Each program
/// answers the value `lo` held at the call. The first resumes the owner from a
/// later operand, the second from the grafted body before it reads the
/// parameter. The counter-factual is the in-place read, which saw the 100 the
/// owner wrote in both.
#[test]
fn a_captured_operand_the_owner_writes_keeps_the_value_it_had_at_the_call() {
    let later = "(defn first-of [x y] x)
                 (defn owner [@lo]
                   (assign lo 0) (yield (fn [d] (first-of lo (d))))
                   (assign lo 100) (yield nil))
                 (def parked (fiber/new (fn [] (owner 1)) |:yield|))
                 (def reader (fiber/resume parked))
                 (reader (fn [] (fiber/resume parked) 2))";
    assert_eq!(run_booted(later), Value::int(0), "{later}");
    let body = "(var parked nil)
                (defn kick [] (fiber/resume parked) 2)
                (defn after-kick [x] (kick) x)
                (defn owner [@lo]
                  (assign lo 0) (yield (fn [] (after-kick lo)))
                  (assign lo 100) (yield nil))
                (assign parked (fiber/new (fn [] (owner 1)) |:yield|))
                ((fiber/resume parked))";
    assert_eq!(run_booted(body), Value::int(0), "{body}");
}

/// Inside a lambda, a binding the lambda captures and the unit writes is
/// bound by a `let`, so the `%add` of the grafted `+` reads the local and
/// never the captured binding. The guards then prove the local, which no call
/// can change. The counter-factual is the in-place read: the guards proved
/// `lo` itself, the call in the second guard ended that fact, and the `%add`
/// was refused.
#[test]
fn a_captured_binding_the_unit_writes_is_bound_by_a_let_inside_a_lambda() {
    let src = "(defn s [@lo] (assign lo 1) (fn [d] (+ lo (d)))) ((s 0) (fn [] 2))";
    let (hir, arena) = compiled(src);
    let reads = intrinsic_operand_names(&hir, &arena, "%add");
    assert_eq!(
        reads.len(),
        2,
        "{src}: the %add reads two locals: {reads:?}"
    );
    assert!(
        !reads.contains(&SymbolId::of("lo")),
        "{src}: the %add reads the local, not lo"
    );
    assert_eq!(run_booted(src), Value::int(3), "{src}");
}
