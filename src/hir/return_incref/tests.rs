//! Unit tests (`super` is the parent impl module).
//!
//! The property under test is the one this pass shares with `mark_tail_calls`:
//! **a `break` targeting a block that is in tail position is itself in tail
//! position**, because the break's value becomes the block's value and the
//! block's value is the function's result. The two passes carry the same
//! `tail_blocks` set for exactly that reason, and an iterative node between the
//! block and the break — `Loop` or `While` — must carry it through even as
//! `in_tail` goes false: the break jumps *past* the loop to the block's exit
//! label (docs/impl/region/mechanism.md § "A break out of a TAIL block carries
//! the return mint").
//!
//! Without the mint the returned value leaves with no owning reference while the
//! release pinned at the exit label still fires, so the caller reads a freed
//! value — the behavioural pin is `region-break-transfer-uaf.lisp`'s tail-loop
//! witnesses.

use super::*;
use crate::hir::expr::HirId;
use crate::symbol::SymbolTable;

/// Compile to canonical (functionalized + ANF) HIR — the point at which
/// `wrap_tail_returns` has run (it is the tail of `anf_lift`).
fn fhir(source: &str) -> Hir {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    let (hir, _arena, _names) =
        crate::pipeline::compile_file_to_fhir(source, &mut symbols, &mut cctx, "<test>")
            .expect("compile");
    hir
}

/// Per `Break` node: how many `Return` mints its value subtree carries, and the
/// variant name of what the first of them wraps. Exactly one mint on every path
/// is the obligation — none means the returned value leaves unowned, two would
/// hand out a reference nothing balances.
fn break_mints(hir: &Hir) -> Vec<(HirId, usize, Option<String>)> {
    fn count_returns(h: &Hir) -> usize {
        let mut n = usize::from(matches!(&h.kind, HirKind::Return { .. }));
        h.for_each_child(|c| n += count_returns(c));
        n
    }
    fn walk(h: &Hir, out: &mut Vec<(HirId, usize, Option<String>)>) {
        if let HirKind::Break { value, .. } = &h.kind {
            out.push((h.id, count_returns(value), first_return_operand_kind(value)));
        }
        h.for_each_child(|c| walk(c, out));
    }
    let mut out = Vec::new();
    walk(hir, &mut out);
    out
}

/// The `HirKind` variant name of the value a `Return` wraps, for the one test
/// that cares *what* got minted.
fn first_return_operand_kind(hir: &Hir) -> Option<String> {
    if let HirKind::Return { value } = &hir.kind {
        return Some(match &value.kind {
            HirKind::Var(_) => "Var".to_string(),
            HirKind::Call { is_tail, .. } => format!("Call{{is_tail:{is_tail}}}"),
            other => format!("{other:?}")
                .split([' ', '(', '{'])
                .next()
                .unwrap_or("?")
                .to_string(),
        });
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_return_operand_kind(c);
        }
    });
    found
}

#[test]
fn break_to_a_tail_block_through_a_loop_wraps_its_value_in_return() {
    // `(fn [] (forever … (break b)))`: `forever` is `(while true …)`, which
    // `analyze_while` wraps in an implicit `:while` block — here the function's
    // tail. The break's value is that block's value, hence the function's result,
    // so it must carry the mint. The `Loop` the functionalize pass leaves between
    // the block and the break is not itself a tail position, but it does not
    // sever the break's.
    let hir = fhir("(fn [] (forever (let [b (string \"s\")] (break b))))");
    let breaks = break_mints(&hir);
    assert_eq!(breaks.len(), 1, "expected one break, got {breaks:?}");
    assert_eq!(
        breaks[0].1, 1,
        "the break's value is the function's returned value but carries {} \
         Return mints — with none it leaves with no owning reference while the \
         release at the block's exit label still fires",
        breaks[0].1,
    );
}

#[test]
fn break_to_a_tail_block_through_nested_loops_wraps_its_value_in_return() {
    // The propagation is not depth-limited: two loops between the tail block and
    // the break, with the break targeting the OUTER block by name.
    let hir = fhir(
        "(fn [] (block :out \
           (forever (forever (let [b (string \"s\")] (break :out b))))))",
    );
    let breaks = break_mints(&hir);
    assert_eq!(breaks.len(), 1, "expected one break, got {breaks:?}");
    assert_eq!(
        breaks[0].1, 1,
        "a break out of nested loops to the function's tail block carries {} \
         Return mints, not 1",
        breaks[0].1,
    );
}

#[test]
fn break_to_an_interior_block_through_a_loop_takes_no_mint() {
    // The control that isolates tail-ness from shape: the same loop and break,
    // but the block's value is consumed by a `let` binding, so nothing crosses
    // the function frontier. Minting here would hand out a reference no caller
    // balances — a per-call leak.
    let hir = fhir("(let [r (forever (break (string \"s\")))] (%string? r))");
    let breaks = break_mints(&hir);
    assert_eq!(breaks.len(), 1, "expected one break, got {breaks:?}");
    assert_eq!(
        breaks[0].1, 0,
        "a break to an INTERIOR block minted {} return references nothing \
         balances",
        breaks[0].1,
    );
}

#[test]
fn a_call_in_break_position_is_minted_once_through_its_anf_name() {
    // The mint is emitted exactly once per returned value (memory.md § Settled).
    // ANF names a call's result, so the mint lands on the NAME — balanced by that
    // binding's own `decref_point` — not a second time around the call itself.
    let hir = fhir("(defn k [x] x) (fn [] (forever (break (k (string \"s\")))))");
    let breaks = break_mints(&hir);
    assert_eq!(breaks.len(), 1, "expected one break, got {breaks:?}");
    assert_eq!(
        breaks[0].1, 1,
        "a call in break position to a tail block carries {} Return mints, not 1",
        breaks[0].1,
    );
    assert_eq!(
        breaks[0].2.as_deref(),
        Some("Var"),
        "the mint must land on the ANF name of the call's result, so the \
         binding's own release balances it (breaks={breaks:?})",
    );
}
