// audited: 2026-10-06
//! Where the lowerer puts each region's release, one submodule per question.
//!
//! docs/impl/region/rules.md
//!
//! - `emission` — that a release is emitted at all, and at which `decref_point`.
//! - `parkmint` — the reference a park mints for a payload the emitting body
//!   borrows (docs/impl/region/park.md).
//! - `frametables` — that the abandoned-frame release tables name exactly the
//!   routes the emitter wrote.
//! - `order` — the order releases take when several share one decref_point
//!   (docs/impl/region/rules.md Rule 4).
//! - `determinism` — that the analysis and the release order it drives are a
//!   pure function of the source.
//! - `cellslots` — that a compiled capture cell owns its region slot, and that
//!   its init is released after the store into it.
//! - `frameexit` — the release a frame owes on the way out, by value route or
//!   by region id, the callee release a tail call defers, and the borrowed
//!   arguments a tail call names.
//! - `cellexit` — the same placement for an env cell's box, and the reads
//!   through the cell a move of its release must not cross.
//! - `armexit` — the replica each frame-exiting branch arm takes of a release
//!   emitted past the merge.
//! - `breakexit` — the replica a `break` leaves at the end of the block it
//!   leaves, for the release its jump passes over.
//! - `arms` — releases across branch arms: a tail-calling arm must not hold back
//!   its falling-through siblings, and a re-storable capture cell's slot is not
//!   a release route.
//! - `shortcircuit` — the same placement across the branch `and`/`or` lower to,
//!   whose arms are their operands.
//! - `restpattern` — the collection a rest pattern built, whose release route is
//!   a parked slot rather than the slot a call passes.

// Re-glob the parent's test imports so each submodule can `use super::*;`.
use super::*;
use crate::lir::code::BlockRef;

mod armexit;
mod arms;
mod breakexit;
mod cellexit;
mod cellslots;
mod determinism;
mod emission;
mod frameexit;
mod frametables;
mod order;
mod parkmint;
mod restpattern;
mod shortcircuit;

/// The local slots each block of `func` releases by value, tagged with whether
/// that block ends in a frame-replacing `TailCall`.
///
/// A branch whose arms do not all tail-call is read here rather than through
/// `branch_arm_release_slots`, which needs one `TailCall` per arm: what these pins
/// ask is whether the release reaches the arm that FALLS THROUGH, so the merge
/// block — which makes no tail call at all — is the block that has to carry it.
fn released_slots_by_block(func: &LirOwned) -> Vec<(bool, Vec<u16>)> {
    func.view()
        .blocks()
        .map(|b| {
            let exits = b.instrs().any(|i| matches!(i, InstrRef::TailCall { .. }));
            (exits, value_released(b.instrs()))
        })
        .collect()
}

/// Every release by value in a run of instructions, in order, as its index in
/// the run and the local slot it releases: each `DecrefValueRegion` whose
/// register a `LoadLocal` earlier in the run filled.
fn value_releases<'a>(instrs: impl IntoIterator<Item = InstrRef<'a>>) -> Vec<(usize, u16)> {
    let mut from_slot: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
    let mut out = Vec::new();
    for (idx, i) in instrs.into_iter().enumerate() {
        match i {
            InstrRef::LoadLocal { dst, slot } => {
                from_slot.insert(dst, slot);
            }
            InstrRef::DecrefValueRegion { src } => {
                if let Some(&slot) = from_slot.get(&src) {
                    out.push((idx, slot));
                }
            }
            _ => {}
        }
    }
    out
}

/// The local slots a run of instructions releases by value, in order.
fn value_released<'a>(instrs: impl IntoIterator<Item = InstrRef<'a>>) -> Vec<u16> {
    value_releases(instrs)
        .into_iter()
        .map(|(_, slot)| slot)
        .collect()
}

/// The index of a block's first `TailCall`, if it makes one.
fn tail_call_at(b: &BlockRef<'_>) -> Option<usize> {
    b.instrs()
        .position(|i| matches!(i, InstrRef::TailCall { .. }))
}

/// Every block of `func` that holds a `TailCall`, in order, with the call's
/// index in it.
fn tail_call_blocks_of(func: &LirOwned) -> impl Iterator<Item = (BlockRef<'_>, usize)> + '_ {
    func.view()
        .blocks()
        .filter_map(|b| Some((b, tail_call_at(&b)?)))
}

/// Every block of every function in `module` that holds a `TailCall`, entry
/// first, with the call's index in it.
fn tail_call_blocks(module: &FrozenModule) -> impl Iterator<Item = (BlockRef<'_>, usize)> + '_ {
    functions(module).flat_map(tail_call_blocks_of)
}

/// The first block of any function in `module` that holds a `TailCall`, with
/// the call's index in it.
fn first_tail_call_block(module: &FrozenModule) -> Option<(BlockRef<'_>, usize)> {
    tail_call_blocks(module).next()
}

/// The indices of a block's instructions that `pred` accepts.
fn positions(b: &BlockRef<'_>, pred: impl Fn(&InstrRef<'_>) -> bool) -> Vec<usize> {
    b.instrs()
        .enumerate()
        .filter(|(_, i)| pred(i))
        .map(|(idx, _)| idx)
        .collect()
}

/// The first function whose blocks include both a `TailCall`-bearing one and one
/// without — a branch where only some arms leave through a callee.
fn mixed_exit_function(module: &FrozenModule) -> Vec<(bool, Vec<u16>)> {
    std::iter::once(&module.entry)
        .chain(module.closures.iter())
        .map(released_slots_by_block)
        .find(|blocks| {
            blocks.iter().any(|(exits, _)| *exits)
                && blocks.iter().any(|(exits, s)| !*exits && !s.is_empty())
        })
        .unwrap_or_default()
}
