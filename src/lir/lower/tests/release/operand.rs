// audited: 2026-09-23
// docs/impl/region/relocate.md
//! What a tail-call operand hands the call: an allocating `%`-op's own region, never the operands it holds by count.

use super::*;

/// The block of `module`'s closures that makes a frame-replacing tail call:
/// the slots its `%pair` read its tail operand from, and the slots it releases
/// by value before the `TailCall`.
fn pair_tail_and_early_releases(module: &crate::lir::LirModule) -> (Option<u16>, Vec<u16>) {
    let block = module
        .closures
        .iter()
        .flat_map(|f| f.blocks.iter())
        .find(|b| {
            b.instructions
                .iter()
                .any(|i| matches!(i.instr, LirInstr::TailCall { .. }))
        })
        .expect("the recursive arm lowers to a TailCall");
    let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
    let mut pair_tail = None;
    let mut released = Vec::new();
    for i in &block.instructions {
        match &i.instr {
            LirInstr::LoadLocal { dst, slot } => {
                from_slot.insert(*dst, *slot);
            }
            LirInstr::List { tail, .. } => pair_tail = from_slot.get(tail).copied(),
            LirInstr::DecrefValueRegion { src } => released.extend(from_slot.get(src)),
            LirInstr::TailCall { .. } => break,
            _ => {}
        }
    }
    (pair_tail, released)
}

/// `step` hands its tail call a fresh pair, and the pair holds `acc` by a count
/// of its own. So the frame's release of `acc` is owed before the call, like
/// any release of a region the call does not receive.
///
/// The counter-factual: the exemption descended the `%pair` into its operands,
/// read `acc` as handed to the call, and left its release after the
/// `TailCall`, where the closure path never runs it. Each call stranded the
/// accumulator it displaced — the oracle's `recur-acc-return` probe, once
/// `pair` grafts to `%pair`.
#[test]
fn an_accumulator_a_pair_holds_is_released_before_the_tail_call() {
    let module = compile_to_lir(
        "(begin (def step (fn (i n acc) (if (%lt i n) (step (%add i 1) n (%pair i acc)) acc))) \
                (step 0 4 ()))",
    );
    let (pair_tail, released) = pair_tail_and_early_releases(&module);
    let acc = pair_tail.expect("the %pair reads acc from its slot");
    assert!(
        released.contains(&acc),
        "acc (slot {acc}) is not released before the TailCall; released before it: \
         {released:?}",
    );
}
