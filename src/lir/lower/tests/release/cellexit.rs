// audited: 2026-10-06
//! An env cell's box release past a frame-replacing tail call: carried ahead, compensated per arm, never across a read.
//!
//! docs/impl/region/mechanism.md
//! docs/impl/region/cells.md

use super::*;

/// Position of the first `TailCall` in the first block that has one AND releases
/// a cell there, with the indices of that block's `DecrefCellRegion`s — the
/// env-cell twin of the frame-exit value-route reading.
///
/// A block with no cell release is skipped rather than returned: its empty
/// release list would satisfy a placement assertion in either direction, so
/// returning it would let a pin pass while measuring nothing.
fn tail_call_cell_release_layout(module: &FrozenModule) -> Option<(usize, Vec<usize>)> {
    tail_call_blocks(module).find_map(|(b, at)| {
        let releases = positions(&b, |i| matches!(i, InstrRef::DecrefCellRegion { .. }));
        (!releases.is_empty()).then_some((at, releases))
    })
}

/// Every block of `module` that makes no `TailCall`.
fn fallthrough_blocks(module: &FrozenModule) -> impl Iterator<Item = BlockRef<'_>> + '_ {
    functions(module)
        .flat_map(|f| f.view().blocks())
        .filter(|b| tail_call_at(b).is_none())
}

/// For each block that makes no `TailCall` and does release a cell, the index of
/// its last `LoadCapture` (the arm's read through the cell, `None` when it makes
/// none) beside the indices of its `DecrefCellRegion`s.
///
/// A `tail`-route release must land AFTER the arm's read; a `head`-route one lands
/// at the arm's head, before any read there is.
fn fallthrough_cell_read_and_release(module: &FrozenModule) -> Vec<(Option<usize>, Vec<usize>)> {
    fallthrough_blocks(module)
        .filter_map(|b| {
            let releases = positions(&b, |i| matches!(i, InstrRef::DecrefCellRegion { .. }));
            if releases.is_empty() {
                return None;
            }
            let read = positions(&b, |i| matches!(i, InstrRef::LoadCapture { .. }))
                .last()
                .copied();
            Some((read, releases))
        })
        .collect()
}

/// The `DecrefCellRegion` counts of the blocks that make no `TailCall` at all —
/// where a branch arm's head compensation lands when its sibling leaves through a
/// callee.
fn cell_releases_in_fallthrough_blocks(module: &FrozenModule) -> Vec<usize> {
    fallthrough_blocks(module)
        .map(|b| {
            b.instrs()
                .filter(|i| matches!(i, InstrRef::DecrefCellRegion { .. }))
                .count()
        })
        .collect()
}

#[test]
fn reassigned_env_cell_release_precedes_the_frame_replacing_tail_call() {
    // `c` is a captured local, so `populate_env` mints its cell box once per
    // activation and the box's `DecrefCellRegion` lands in the dead block. It is
    // hoisted even though `c` is REASSIGNED: the mutated refusal is compensation's
    // release-ROUTE one, and this release names the box (`LoadCaptureRaw`), which
    // an `assign` never repoints — it writes the cell's content
    // (docs/impl/region/mechanism.md § "A mutated holder poisons its value route,
    // not its cell box"; the `fresh-env-cell` probe).
    let module = compile_to_lir(
        "(begin (def f (fn () (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (g)))) (f))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "the reassigned env cell's release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path, one box \
         stranded per activation",
    );
}

/// Every place a block frees an env cell before something in that same block reads
/// through it, as `(env index, release position, read position)`.
///
/// One reading serves both directions the pin needs: the release's own
/// `LoadCaptureRaw` sits immediately BEFORE it, so a well-ordered block reports
/// nothing, and any tuple here is a `DecrefCellRegion` that freed the box under a
/// later unwrap of the same index.
fn cell_release_inversions(module: &FrozenModule) -> Vec<(u16, usize, usize)> {
    let mut out = Vec::new();
    for b in functions(module).flat_map(|f| f.view().blocks()) {
        let mut from_index: rustc_hash::FxHashMap<Reg, u16> = rustc_hash::FxHashMap::default();
        let mut reads: Vec<(u16, usize)> = Vec::new();
        let mut freed: Vec<(u16, usize)> = Vec::new();
        for (idx, i) in b.instrs().enumerate() {
            match i {
                InstrRef::LoadCapture { dst, index } | InstrRef::LoadCaptureRaw { dst, index } => {
                    from_index.insert(dst, index);
                    reads.push((index, idx));
                }
                InstrRef::DecrefCellRegion { src } => {
                    freed.extend(from_index.get(&src).map(|&index| (index, idx)));
                }
                _ => {}
            }
        }
        for (index, at) in freed {
            out.extend(
                reads
                    .iter()
                    .filter(|&&(i, r)| i == index && r > at)
                    .map(|&(_, r)| (index, at, r)),
            );
        }
    }
    out
}

#[test]
fn a_cell_release_declines_a_move_across_a_read_through_it() {
    // The closure-as-module shape: `a` is a captured def, and the body's last form
    // is a struct literal — a NATIVE tail call, so the dispatch loop falls through
    // into the block after the `TailCall` and runs everything the lowerer put
    // there.
    //
    // The trap is that `a`'s two releases are two REGIONS, and the relocation
    // answers per region. `ptr/from-int` declares `RegionEffect::Immediate`, so the
    // walk records no result region for the init and no binding names it — the
    // frame-held admission refuses it for want of a holder, and its
    // `DecrefValueRegion` keeps its place in the dead block. The env CELL is
    // admitted on the binding's own verdict and moves ahead of the call. That value
    // release loads the box RAW and unwraps it, so it reads the page the cell
    // release frees (docs/impl/region/mechanism.md § "A move that crosses a read
    // through the cell it frees is declined").
    //
    // The counter-factual: asserting only that some cell release precedes the
    // `TailCall` passes on this witness while the box is freed under its own
    // reader, because the move is exactly what the assertion asks for.
    let module = compile_to_lir(
        "(begin (def m (fn () \
           (def a (ptr/from-int 0)) \
           (def p (fn () a)) \
           {:p p})) \
         (m))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r > at),
        "the env cell's release was moved ahead of the TailCall \
         (at={at}, releases={releases:?}) while the release routed through that \
         cell stayed behind — the unwrap then reads a freed page",
    );
    let inversions = cell_release_inversions(&module);
    assert!(
        inversions.is_empty(),
        "a cell is freed before a read through it; (env index, release, read) \
         positions = {inversions:?}",
    );
}

#[test]
fn escaping_holder_env_cell_release_stays_after_the_tail_call() {
    // The decline face: the closure holding the cell is STORED into an aggregate
    // before the body tail-calls it, so escape's capture facet marks `c` escaping by a
    // CONTAINMENT facet and both admissions refuse the box — the aggregate holds the
    // closure through a hold no seam at the point counts. Only the mutated refusal is
    // scoped to the value route; a containment facet no edge at the point replaces
    // still refuses, and the release keeps its place in the dead block.
    let module = compile_to_lir(
        "(begin (def @sink nil) (def f (fn () (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] \
           (begin (assign sink (%pair g nil)) (g))))) \
         (f))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r > at),
        "an escaping holder's env cell was hoisted ahead of the TailCall \
         (at={at}, releases={releases:?}) — the closure leaves carrying the cell",
    );
}

#[test]
fn yielded_holder_env_cell_release_precedes_the_frame_replacing_tail_call() {
    // The admitted face beside it: the closure holding the cell crosses the FIBER
    // frontier rather than a containment one. Every seam that hands a value to another
    // fiber counts a reference of its own — the park's `EmitEscape` retain going out,
    // the resume value's own mint coming back — so the crossing is not the uncounted
    // second holder the admission guards against, and the box's release is hoisted
    // ahead of the `TailCall` like any other
    // (docs/impl/region/mechanism.md § "A fiber crossing is a counted holder too").
    let module = compile_to_lir(
        "(begin (def f (fn () (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (begin (emit :yield g) (g))))) \
         (fiber/new f |:yield|))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().any(|&r| r < at),
        "a yielded holder's env cell release is still emitted after the TailCall \
         (at={at}, releases={releases:?}) — dead on the closure path, one box \
         stranded per activation",
    );
}

#[test]
fn a_falling_through_arm_head_releases_the_env_cell_its_sibling_relocated() {
    // `(if t (g) 0)` — the box's one `DecrefCellRegion` relocates into the arm that
    // tail-calls `g`, so the arm that falls through to the merge would release
    // nothing. That arm names `c` nowhere, so branch compensation's head route
    // covers it; the two are mutually exclusive by arm structure, which is what a
    // cell release needs because it leaves no nil-stamp to make a replica no-op
    // (docs/impl/region/mechanism.md § "A compensating release of an env cell names
    // the box, not the holder's slot").
    let module = compile_to_lir(
        "(begin (def f (fn (t) (def @c 0) \
         (let [g (fn () (assign c (%add c 1)) c)] (if t (g) 0)))) (f false))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r < at),
        "the tail-calling arm must keep its relocated cell release ahead of the \
         TailCall (at={at}, releases={releases:?})",
    );
    let fallthrough = cell_releases_in_fallthrough_blocks(&module);
    assert!(
        fallthrough.contains(&1),
        "some block that makes no tail call must release the cell exactly once — \
         the falling-through arm's head compensation; per-block counts={fallthrough:?}",
    );
    assert!(
        fallthrough.iter().all(|&n| n <= 1),
        "no block may release the cell twice; per-block counts={fallthrough:?}",
    );
}

#[test]
fn a_reading_arm_tail_releases_the_env_cell_its_sibling_relocated() {
    // `(if t c (g))` — the same relocation, and the sibling arm READS `c`. The
    // capture-use of `c` resolves through `g`'s last use, so the `decref_point`
    // follows the call rather than the read and lands in the LATER arm. A head
    // release on the reading arm would free the box under that read, so the arm
    // takes the `tail` route instead: one `DecrefCellRegion` after its
    // `LoadCapture`. The route's same-node retain is a claim about the value the
    // holder names; the box's own holders are the frame's env slot and the
    // capturer's counted edge (docs/impl/region/mechanism.md § "A compensating
    // release of an env cell names the box, not the holder's slot").
    let module = compile_to_lir(
        "(begin (def f (fn (n t) (def @c n) \
         (let [g (fn () c)] (if t c (g))))) (f 1 true))",
    );
    let (at, releases) =
        tail_call_cell_release_layout(&module).expect("the body lowers to a TailCall");
    assert!(
        releases.iter().all(|&r| r < at),
        "the tail-calling arm must keep its relocated cell release ahead of the \
         TailCall (at={at}, releases={releases:?})",
    );
    let arms = fallthrough_cell_read_and_release(&module);
    assert!(
        arms.iter()
            .any(|(read, rel)| read.is_some_and(|r| rel.iter().all(|&d| d > r)) && rel.len() == 1),
        "the reading arm must release the box exactly once, after its own read \
         through the cell; per-block (last LoadCapture, DecrefCellRegion)={arms:?}",
    );
    assert!(
        arms.iter().all(|(_, rel)| rel.len() <= 1),
        "no block may release the cell twice; per-block reads/releases={arms:?}",
    );
}
