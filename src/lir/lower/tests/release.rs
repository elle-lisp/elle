// audited: 2026-11-23
//! Where the lowerer puts each region's release, one submodule per question.
//!
//! docs/impl/region/rules.md

// Re-glob the parent's test imports so each submodule can `use super::*;`.
use super::*;
use crate::lir::code::BlockRef;

/// The function in `module` whose name is `name`.
fn named<'m>(module: &'m FrozenModule, name: &str) -> &'m LirOwned {
    functions(module)
        .find(|f| f.view().name() == Some(name))
        .unwrap_or_else(|| panic!("a function named {name}"))
}

mod armexit;
mod armpoint;
mod arms;
mod breakexit;
mod cellexit;
mod cellinit;
mod cellslots;
mod determinism;
mod emission;
mod frameexit;
mod frametables;
mod handoff;
mod operand;
mod order;
mod parkmint;
mod postponed;
mod restpattern;
mod retained;
mod route;
mod shortcircuit;

/// For the first function with two `TailCall`-bearing blocks — a branch whose
/// arms each make one — the local slots each block releases BEFORE its call and
/// those it releases after.
///
/// Reading by SLOT rather than by instruction count is what makes these pins
/// specific: an arm carries the replicated release of *every* region the merge
/// releases, so "some release precedes the call" says nothing about which.
fn branch_arm_release_slots(module: &FrozenModule) -> Vec<(Vec<u16>, Vec<u16>)> {
    for f in functions(module) {
        let arms: Vec<(Vec<u16>, Vec<u16>)> = tail_call_blocks_of(f)
            .map(|(b, at)| {
                let (mut before, mut after) = (Vec::new(), Vec::new());
                for (idx, slot) in value_releases(b.instrs()) {
                    if idx < at {
                        before.push(slot);
                    } else {
                        after.push(slot);
                    }
                }
                (before, after)
            })
            .collect();
        if arms.len() >= 2 {
            return arms;
        }
    }
    Vec::new()
}

/// The local slot the first `MakeClosure` of the first function that leaves
/// through a `TailCall` is stored into.
///
/// A `letrec`-bound closure's release names that slot, and reading the slot off
/// the emission rather than counting locals is what keeps the pins that read it
/// specific: which index a binder gets depends on how many parameters and ANF
/// temporaries precede it.
fn tail_calling_functions_closure_slot(module: &FrozenModule) -> Option<u16> {
    for f in functions(module) {
        if tail_call_blocks_of(f).next().is_none() {
            continue;
        }
        let mut made: Option<Reg> = None;
        for i in flat_instrs(f) {
            match i {
                InstrRef::MakeClosure { dst, .. } => made = Some(dst),
                InstrRef::StoreLocal { slot, src } if Some(src) == made => return Some(slot),
                _ => {}
            }
        }
    }
    None
}

/// The local slots each block of `func` releases by value, tagged with whether
/// that block ends in a frame-replacing `TailCall`.
///
/// A branch whose arms do not all tail-call is read here rather than through
/// `branch_arm_release_slots`, which needs one `TailCall` per arm. These pins ask
/// whether the release reaches the arm that FALLS THROUGH. So the merge block,
/// which makes no tail call at all, is the block that has to carry it.
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

/// Every value route in `instrs` — a `LoadLocal` feeding the next
/// instruction's `DecrefValueRegion` — as its slot, and whether a `StoreLocal`
/// stamps that slot within the two instructions after the release. The stamp
/// is `StoreLocal slot <nil>`, and materializing the nil takes an instruction
/// of its own, so the search looks past it.
fn stamped_value_routes(instrs: &[InstrRef<'_>]) -> Vec<(u16, bool)> {
    (0..instrs.len())
        .filter_map(|i| {
            let (InstrRef::LoadLocal { dst, slot }, InstrRef::DecrefValueRegion { src }) =
                (instrs.get(i)?, instrs.get(i + 1)?)
            else {
                return None;
            };
            if dst != src {
                return None;
            }
            let stamped = instrs[i + 2..]
                .iter()
                .take(2)
                .any(|n| matches!(n, InstrRef::StoreLocal { slot: back, .. } if back == slot));
            Some((*slot, stamped))
        })
        .collect()
}

/// Each block of `func` that releases local `slot` by value ahead of any tail
/// call it makes, by index, with whether it stamps the slot after the release.
fn early_releases(func: &LirOwned, slot: u16) -> Vec<(usize, BlockRef<'_>, bool)> {
    let mut early = Vec::new();
    for (n, b) in func.view().blocks().enumerate() {
        let cut = tail_call_at(&b).unwrap_or(usize::MAX);
        if !value_releases(b.instrs())
            .into_iter()
            .any(|(idx, s)| s == slot && idx < cut)
        {
            continue;
        }
        let instrs: Vec<InstrRef<'_>> = b.instrs().take(cut).collect();
        let stamped = stamped_value_routes(&instrs).contains(&(slot, true));
        early.push((n, b, stamped));
    }
    early
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
