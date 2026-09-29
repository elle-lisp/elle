// audited: 2026-09-29
//! A compiled capture cell gets a region slot of its own, and its init is released only after the store into it.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/model.md

use super::*;

#[test]
fn preallocated_capture_cells_get_distinct_regions_each_released() {
    // One allocation execution per static slot between drops
    // (docs/impl/region/model.md). `lower_begin` pre-allocates one
    // `MakeCaptureCell` per captured top-level binding; emitting two cells
    // against ONE slot orphans the first cell's physical region (the runtime
    // mints fresh per execution and overwrites the activation mapping, so the
    // slot's single `DecrefRegion` only ever releases the last cell)
    // (tests/impl/region-capture-cell-shared-slot-leak.lisp).
    //
    // Shape: TWO captured bindings — `cap-a` (captured by `cap-b`'s inner
    // letrec lambda) and `cap-b` (captured by `cap-d`) — so the Begin pre-pass
    // emits two MakeCaptureCells. Assert each carries its own region slot and
    // each slot has a matching plain `DecrefRegion`.
    let module = compile_to_lir(
        "(begin \
           (def cap-a (fn () 1)) \
           (def cap-b (fn () (cap-a))) \
           (def cap-d (fn () (cap-b))) \
           nil)",
    );
    //
    // These `def`s live in a LOCAL clique (inside the stub letrec body, all discarded:
    // `cap-d ⊇ cap-b ⊇ cap-a`), so the ownership forest reclaims them as a unit —
    // each cell is capture-adopted into its holding closure (`closure ⊇ cell`) and its
    // content adopted into it (`cell ⊇ content`) via `AdoptCellRegion`, and the outermost
    // closure's subtree drop frees the whole clique. An adopted cell's own decref is
    // therefore SUPPRESSED. So each cell region is released EITHER by its own
    // `DecrefRegion` (the Shared baseline) OR by adoption (an `AdoptCellRegion` links it
    // into a subtree) — never silently dropped, and never sharing a slot.
    fn collect(
        func: &LirFunction,
        cells: &mut Vec<StaticRegion>,
        decrefs: &mut Vec<StaticRegion>,
        adopt_cells: &mut usize,
    ) {
        for b in &func.blocks {
            for i in &b.instructions {
                match &i.instr {
                    LirInstr::MakeCaptureCell { region, .. } => cells.push(*region),
                    LirInstr::DecrefRegion { region_id } => decrefs.push(*region_id),
                    LirInstr::AdoptCellRegion { .. } => *adopt_cells += 1,
                    _ => {}
                }
            }
        }
    }
    let mut cells = Vec::new();
    let mut decrefs = Vec::new();
    let mut adopt_cells = 0usize;
    collect(&module.entry, &mut cells, &mut decrefs, &mut adopt_cells);
    for c in &module.closures {
        collect(c, &mut cells, &mut decrefs, &mut adopt_cells);
    }
    assert!(
        cells.len() >= 2,
        "expected the Begin pre-pass to emit two MakeCaptureCells (cap-a, cap-b); got {cells:?}",
    );
    for (i, a) in cells.iter().enumerate() {
        for b in cells.iter().skip(i + 1) {
            assert_ne!(
                a, b,
                "two MakeCaptureCells share one region slot — the runtime \
                 overwrites the slot's activation mapping per alloc, so the \
                 slot's single DecrefRegion frees only the last cell and every \
                 earlier cell's region leaks (cells={cells:?})",
            );
        }
    }
    // The clique is adopted (a local, non-escaping closure chain), so its cells reclaim
    // via `AdoptCellRegion` + the root's subtree drop rather than per-cell `DecrefRegion`s.
    assert!(
        adopt_cells > 0,
        "the local closure clique cap-d ⊇ cap-b ⊇ cap-a must reclaim by adoption \
         (an AdoptCellRegion links each cell into its holder's subtree); got none",
    );
    for cell in &cells {
        assert!(
            decrefs.contains(cell) || adopt_cells > 0,
            "MakeCaptureCell region {cell:?} is neither released by its own DecrefRegion \
             nor adopted into a subtree — its initial reference would leak \
             (decrefs={decrefs:?}, adopt_cells={adopt_cells})",
        );
    }
}

#[test]
fn letrec_init_release_fires_after_cell_store() {
    // A letrec init's region releases must be emitted AFTER the value is stored into
    // the binding's slot/cell, exactly as `lower_let` defers them. The counterfactual:
    // a captured binding with no surviving uses (its references resolve to a later
    // duplicate) keeps its closure region's `decref_point` at the init node itself, so
    // without the deferral the `DecrefRegion` lands between `MakeClosure` and the cell
    // store — the closure is freed before `UpdateCapture` increfs it, the cell dangles,
    // and the teardown scan misattributes the reused pages as a phantom decref.
    //
    // The shape: `gg` is captured by the EARLIER lambda `ff` (forward ref), so
    // `gg`'s only use site is structurally before its own init — the
    // binding-chain extension cannot move its region's decref_point past the
    // init node, and only the deferral keeps the release after the store.
    let module = compile_to_lir(
        "(letrec [ff (fn () gg) \
                  gg (fn (x) x)] \
           1)",
    );
    fn check(func: &LirFunction) {
        for b in &func.blocks {
            // Track, per closure-producing register, the MakeClosure's
            // region; flag a plain DecrefRegion of that region appearing
            // before the register is consumed by a store.
            let mut pending: Vec<(Reg, StaticRegion)> = Vec::new();
            for (idx, i) in b.instructions.iter().enumerate() {
                match &i.instr {
                    LirInstr::MakeClosure { dst, region, .. } => {
                        pending.push((*dst, *region));
                    }
                    LirInstr::StoreCaptureCell { value, .. }
                    | LirInstr::StoreLocal { src: value, .. } => {
                        pending.retain(|(r, _)| r != value);
                    }
                    LirInstr::DecrefRegion { region_id } => {
                        assert!(
                            !pending.iter().any(|(_, reg)| reg == region_id),
                            "DecrefRegion({region_id:?}) at instr {idx} fires between a \
                             MakeClosure into that region and the store that consumes \
                             the closure — the value is freed before the cell's \
                             incref",
                        );
                    }
                    _ => {}
                }
            }
        }
    }
    check(&module.entry);
    for c in &module.closures {
        check(c);
    }
}
