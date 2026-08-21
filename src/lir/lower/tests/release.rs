// ── Region-lifecycle: decref/release emission ────────────────────
//
// Where the lowerer puts each region's release, split by the question each
// group answers:
//
// - `emission` — that a release is emitted at all, and at which `decref_point`.
// - `order` — the order releases take when several share one decref_point
//   (docs/impl/region/rules.md Rule 4).
// - `frameexit` — the release a frame owes on the way out.
// - `arms` — releases across branch arms: a tail-calling arm must not hold back
//   its falling-through siblings, and a re-storable capture cell's slot is not
//   a release route.

// Re-glob the parent's test imports so each submodule can `use super::*;`.
use super::*;

mod arms;
mod emission;
mod frameexit;
mod order;
/// The local slots each block of `func` releases by value, tagged with whether
/// that block ends in a frame-replacing `TailCall`.
///
/// A branch whose arms do not all tail-call is read here rather than through
/// `branch_arm_release_slots`, which needs one `TailCall` per arm: what these pins
/// ask is whether the release reaches the arm that FALLS THROUGH, so the merge
/// block — which makes no tail call at all — is the block that has to carry it.
fn released_slots_by_block(func: &LirFunction) -> Vec<(bool, Vec<u16>)> {
    func.blocks
        .iter()
        .map(|b| {
            let exits = b
                .instructions
                .iter()
                .any(|i| matches!(i.instr, LirInstr::TailCall { .. }));
            let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
            let mut slots = Vec::new();
            for i in &b.instructions {
                match &i.instr {
                    LirInstr::LoadLocal { dst, slot } => {
                        from_slot.insert(*dst, *slot);
                    }
                    LirInstr::DecrefValueRegion { src } => {
                        if let Some(&slot) = from_slot.get(src) {
                            slots.push(slot);
                        }
                    }
                    _ => {}
                }
            }
            (exits, slots)
        })
        .collect()
}

/// The first function whose blocks include both a `TailCall`-bearing one and one
/// without — a branch where only some arms leave through a callee.
fn mixed_exit_function(module: &crate::lir::LirModule) -> Vec<(bool, Vec<u16>)> {
    std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .map(released_slots_by_block)
        .find(|blocks| {
            blocks.iter().any(|(exits, _)| *exits)
                && blocks.iter().any(|(exits, s)| !*exits && !s.is_empty())
        })
        .unwrap_or_default()
}
