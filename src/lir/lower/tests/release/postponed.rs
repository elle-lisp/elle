// audited: 2026-11-13
//! The release an operand arm postpones to its tail call, where a later operand reads the region.
//!
//! docs/impl/region/operand.md

use super::*;

/// Each block of `func` that copies local `from` into local `to`, by index,
/// with whether the copy is the block's last two instructions: the end of the
/// arm, ahead of its jump to the merge.
fn copies(func: &LirOwned, from: u16, to: u16) -> Vec<(usize, bool)> {
    let mut out = Vec::new();
    for (n, b) in func.view().blocks().enumerate() {
        let instrs: Vec<InstrRef<'_>> = b.instrs().collect();
        for i in 1..instrs.len() {
            let (InstrRef::LoadLocal { dst, slot }, InstrRef::StoreLocal { slot: back, src }) =
                (instrs[i - 1], instrs[i])
            else {
                continue;
            };
            if slot == from && back == to && dst == src {
                out.push((n, i + 1 == instrs.len()));
            }
        }
    }
    out
}

/// The tail operand is a branch whose pair arm hands the call a pair that holds
/// `acc` by its own count, and whose other arm hands the call `acc` itself.
/// The later operand reads `acc` after the arm, so the pair arm cannot release
/// it there. The arm copies `acc`'s slot into a postponed slot instead. One
/// `DecrefPostponed` names both slots, after the later operand's read and
/// before the `TailCall`. Only `acc`'s slot is in the frame's release table, so
/// a frame abandoned between the two runs the release once.
///
/// The counter-factual: the arm refused a region a later operand reads, and
/// the release stayed behind the `TailCall`, where the closure path never runs
/// it. Each kept element stranded the accumulator it displaced.
#[test]
fn an_operand_arm_postpones_what_a_later_operand_reads() {
    for operand in [
        "(if (%lt 0 (first c)) (%pair (first c) acc) acc)",
        "(or (and (%lt 0 (first c)) (%pair (first c) acc)) acc)",
    ] {
        let module = compile_to_lir(&format!(
            "(begin (def keep (fn (c acc n) \
                      (if (empty? c) n (keep (rest c) {operand} (length acc))))) \
                    (keep (list 1 2) () 0))"
        ));
        let keep = module
            .closures
            .iter()
            .find(|f| tail_call_blocks_of(f).next().is_some())
            .expect("the closure making the tail call");
        let (b, at) = tail_call_blocks_of(keep)
            .next()
            .expect("the block making the tail call");
        let instrs: Vec<InstrRef<'_>> = b.instrs().collect();
        let postponed: Vec<(usize, u16, u16)> = instrs
            .iter()
            .enumerate()
            .filter_map(|(idx, i)| match *i {
                InstrRef::DecrefPostponed { slot, route } => Some((idx, slot, route)),
                _ => None,
            })
            .collect();
        // `acc` is the second parameter, hence local slot 1.
        let [(idx, slot, 1)] = postponed.as_slice() else {
            panic!(
                "the tail-call block must run one postponed release of acc (slot 1); \
                 (index, slot, route): {postponed:?}; operand {operand}"
            );
        };
        let read = instrs[..at]
            .iter()
            .rposition(|i| matches!(i, InstrRef::LoadLocal { slot: 1, .. }))
            .expect("the later operand reads acc");
        assert!(
            read < *idx && *idx < at,
            "the postponed release must run after the later operand reads acc (index \
             {read}) and before the TailCall (index {at}); it sits at {idx}; operand \
             {operand}",
        );
        let copied = copies(keep, 1, *slot);
        assert!(
            matches!(copied.as_slice(), [(_, true)]),
            "one arm must end by copying acc into the postponed slot {slot}; (block, at \
             the end): {copied:?}; operand {operand}",
        );
        let early: Vec<usize> = early_releases(keep, 1)
            .into_iter()
            .filter(|(_, b, _)| !matches!(b.terminator(), Terminator::Return(_)))
            .map(|(n, _, _)| n)
            .collect();
        assert!(
            early.is_empty(),
            "blocks {early:?} release acc ahead of the later operand that reads it; \
             operand {operand}",
        );
        let table = keep.view().frame_release_slots();
        assert!(
            table.contains(&1) && !table.contains(slot),
            "the release table must name acc's slot and not the postponed slot {slot}: \
             {table:?}; operand {operand}",
        );
    }
}
