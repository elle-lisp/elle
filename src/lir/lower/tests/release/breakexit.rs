// audited: 2026-09-05
// Placement pins for the relocation point a `break` opens at the end of the
// block it leaves.
//
// docs/impl/region/replicate.md

use super::*;

// ── The break's replica ──────────────────────────────────────────
// A region the loop body ALLOCATES is released once per iteration, so the break
// window refuses to hoist that release to the block's exit label. The iteration
// that BREAKS is then unserved: its value has no successor to displace it and no
// later release to reach it. The close is a replica at the break, which is a
// question of POSITION alone — the counts are identical either way, so only
// where each release sits can tell the two apart.

/// Every stack slot this function releases by value in two or more DISTINCT
/// blocks — the signature of a replicated release, since a slot the lowerer
/// releases once appears in exactly one block.
fn slots_released_in_several_blocks(func: &LirFunction) -> Vec<u16> {
    let mut per_slot: rustc_hash::FxHashMap<u16, rustc_hash::FxHashSet<usize>> =
        rustc_hash::FxHashMap::default();
    for (bi, b) in func.blocks.iter().enumerate() {
        let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        for i in &b.instructions {
            match &i.instr {
                LirInstr::LoadLocal { dst, slot } => {
                    from_slot.insert(*dst, *slot);
                }
                LirInstr::DecrefValueRegion { src } => {
                    if let Some(&slot) = from_slot.get(src) {
                        per_slot.entry(slot).or_default().insert(bi);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out: Vec<u16> = per_slot
        .into_iter()
        .filter(|(_, blocks)| blocks.len() > 1)
        .map(|(slot, _)| slot)
        .collect();
    out.sort_unstable();
    out
}

/// The same reading over every function in the module.
fn replicated_slots(module: &crate::lir::LirModule) -> Vec<u16> {
    std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .flat_map(slots_released_in_several_blocks)
        .collect()
}

#[test]
fn a_cond_clause_body_that_breaks_out_of_a_loop_takes_a_replica() {
    // The reported shape. `msg`'s last use is the second clause's TEST, so the
    // branch-arm window anchors its release at the `cond`'s own merge — a block
    // both breaking clause bodies jump straight past. Without a replica at each
    // break the breaking iteration's region is held to fiber teardown.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def f (fn ()
                  (while true
                    (let [msg (mk)]
                      (cond
                        (= (get msg :type) :data) (break nil)
                        (= (get msg :type) :error) (break nil)
                        true (break nil))))))
                (f))",
    );
    assert!(
        !replicated_slots(&module).is_empty(),
        "the merge release was not replicated into the breaking arms — the \
         breaking iteration's region is stranded once per call",
    );
}

#[test]
fn a_release_past_the_branchs_merge_still_takes_a_replica() {
    // The point has to outlive the branch that carried the break, not die at its
    // merge: here `msg`'s last use is AFTER the `when`, so the release the break
    // jumps over is emitted once the branch has already closed.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def f (fn (k)
                  (while true
                    (let [msg (mk)]
                      (if (= k 1) (break nil) nil)
                      (get msg :n)))))
                (f 1))",
    );
    assert!(
        !replicated_slots(&module).is_empty(),
        "a release emitted past the branch's merge took no replica — the point \
         died with the branch instead of with the block",
    );
}

#[test]
fn the_value_a_break_carries_takes_no_replica() {
    // The exemption, and the over-free face of the same placement. The break
    // TRANSFERS `msg` to its block, so the release is pinned where the block's
    // value is consumed — a point the jump reaches. A replica at the break would
    // free what the block is about to hand its consumer.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def f (fn ()
                  (while true
                    (let [msg (mk)]
                      (cond
                        (= (get msg :type) :error) (break nil)
                        true (break msg))))))
                (f))",
    );
    assert!(
        replicated_slots(&module).is_empty(),
        "the value the break carries was released at the break — the block is \
         about to hand it to its consumer",
    );
}

#[test]
fn a_break_out_of_a_plain_block_takes_no_replica() {
    // No loop, so the window's count argument holds and the release is hoisted
    // to the block's exit label — a point both paths reach. The point is dead by
    // then (the block has closed), so nothing is replicated and the release
    // stays single.
    let module = compile_to_lir(
        "(begin (def mk (fn () {:type :data :n 1}))
                (def f (fn (k)
                  (block (let [msg (mk)]
                           (if (= k 1) (break 1) nil)
                           (get msg :n)))))
                (f 1))",
    );
    assert!(
        replicated_slots(&module).is_empty(),
        "a release the break window already hoisted to the block's exit was \
         replicated as well — one release is owed, not two",
    );
}
