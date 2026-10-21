// audited: 2026-10-21
//! A forward cell is frame-held exactly as its binding is, on every facet that admits and the one that refuses.
//!
//! docs/impl/region/relocate.md
//! docs/impl/region/held.md
//!
//! These pins read the projection directly: the sibling, return and fiber faces
//! it admits, and the containment facet that must still refuse.

use super::*;

/// Every compiled capture cell in `hir`, as (binding, cell region).
fn compiled_cells(info: &RegionInfo) -> Vec<(Binding, Region)> {
    info.begin_cell_regions
        .values()
        .flatten()
        .copied()
        .collect()
}

#[test]
fn frame_held_names_a_sibling_captured_forward_cell() {
    // The everyday non-cycle sibling capture: `go` calls `helper`, `helper` does not
    // call back, so there is no SCC and no merge — `helper` simply keeps a forward
    // cell for `go`'s benefit. Nothing here leaves the frame, so the cell must be
    // frame-held under `helper`'s own verdict. Its holders are `helper`'s holders one
    // indirection out: this frame's slot, plus the counted `closure ⊇ cell` edge
    // `go`'s env took.
    //
    // Neither member is recursive, so the projection is the only route that
    // releases the cell here. A SELF-recursive capturer's cell is claimed by the
    // ownership forest's capture adopt instead (`capture_adopt_edges`, its own
    // decref suppressed), so the relocation never has to reach it.
    let (_hir, _arena, info) = pipeline(
        "(defn h [n] \
           (letrec [helper (fn [x] (when (%not (%int? x)) (error :x)) (%sub x 1)) \
                    go (fn [m] (when (%not (%int? m)) (error :m)) (helper m))] \
             (go n)))",
    );
    let cells = compiled_cells(&info);
    assert_eq!(
        cells.len(),
        1,
        "precondition: exactly one compiled cell — `helper`'s forward cell, kept \
         because sibling `go` captures it; got {cells:?}",
    );
    let (helper, cell) = cells[0];
    let content = info
        .binding_source_regions
        .get(&helper)
        .cloned()
        .unwrap_or_default();
    for r in &content {
        assert!(
            info.frame_held_regions.contains(r),
            "anchor: the binding's own closure region r{} is frame-held (nothing \
             leaves the frame), so the cell's refusal cannot be blamed on the verdict; \
             frame_held={:?}",
            r.0,
            info.frame_held_regions,
        );
    }
    assert!(
        info.frame_held_regions.contains(&cell),
        "the forward cell r{} must be frame-held exactly as its binding {helper:?} is — \
         no route reaches the cell that does not reach the binding; frame_held={:?}",
        cell.0,
        info.frame_held_regions,
    );
}

#[test]
fn frame_held_names_a_returned_capturers_forward_cell() {
    // The RETURN face of the same projection. `go` is handed back, so escape's capture
    // facet carries `helper` out with it, but by the return facet alone, which the
    // admission allows. The callee reaches the cell through the counted `closure ⊇ cell`
    // edge its env took, so the frame's release is not the last one standing. The cell
    // must therefore carry its binding's verdict here too, or the relocation admits the
    // closure and strands the cell that holds it — one region short of the cascade.
    let (_hir, _arena, info) = pipeline(
        "(defn h [n] \
           (letrec [helper (fn [x] (when (%not (%int? x)) (error :x)) (%sub x 1)) \
                    go (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) go (go (helper m))))] \
             (go n)))",
    );
    let cells = compiled_cells(&info);
    assert_eq!(
        cells.len(),
        1,
        "precondition: exactly one compiled cell — `helper`'s forward cell; got {cells:?}",
    );
    let (helper, cell) = cells[0];
    let content = info
        .binding_source_regions
        .get(&helper)
        .cloned()
        .unwrap_or_default();
    for r in &content {
        assert!(
            info.frame_held_regions.contains(r),
            "anchor: the binding's own closure region r{} leaves by the return facet \
             and no other, so it is frame-held and the cell's verdict must match it; \
             frame_held={:?}",
            r.0,
            info.frame_held_regions,
        );
    }
    assert!(
        info.frame_held_regions.contains(&cell),
        "the forward cell r{} leaves by the RETURN facet and no other, exactly as its \
         binding does, so it must be frame-held; frame_held={:?}",
        cell.0,
        info.frame_held_regions,
    );
}

#[test]
fn frame_held_names_a_yielded_capturers_forward_cell() {
    // The FIBER face of the same projection. `go` is YIELDED, so escape's capture facet
    // carries `helper` across the fiber frontier with it. The park counts a reference
    // for that crossing: its `EmitEscape` retain going out, the resume value's own mint
    // coming back. So it is not the uncounted borrow the admission guards against.
    // The cell must therefore carry its binding's verdict here as it does under the
    // return facet; `frame_held_refuses_a_forward_cell_whose_binding_is_stored` below
    // is the refusal that keeps the projection honest.
    let mut symbols = SymbolTable::new();
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let (hir, arena) = compile_fhir(
        "(fiber/new (fn () \
           (letrec [helper (fn [x] (when (%not (%int? x)) (error :x)) (%sub x 1)) \
                    go (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) :done (go (helper m))))] \
             (begin (yield go) (go 3)))) \
           |:yield|)",
        &mut symbols,
    );
    let info = analyze_regions_with(&hir, &arena, pc.call_classification);
    let cells = compiled_cells(&info);
    assert_eq!(
        cells.len(),
        1,
        "precondition: exactly one compiled cell — `helper`'s forward cell; got {cells:?}",
    );
    let (_helper, cell) = cells[0];
    assert!(
        info.frame_held_regions.contains(&cell),
        "a forward cell r{} whose binding crosses the FIBER frontier must carry its \
         binding's verdict — the crossing counts its own reference, so it is no \
         uncounted second holder; frame_held={:?}",
        cell.0,
        info.frame_held_regions,
    );
}

#[test]
fn frame_held_refuses_a_forward_cell_whose_binding_is_stored() {
    // The counterfactual: the projection carries the binding's verdict, so it must
    // carry a REFUSAL too. `helper` is handed to `git`, a `Mixed` native that stores
    // it uncounted, so escape marks it escaping by a CONTAINMENT facet — a holder no
    // seam at the point counts. The cell must be refused there and its release must
    // keep the baseline. A `%pair` would not do: it counts its hold on `helper`.
    let mut symbols = SymbolTable::new();
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&meta);
    let (hir, arena) = compile_fhir(
        "(defn h [n] \
           (letrec [helper (fn [x] (when (%not (%int? x)) (error :x)) (%sub x 1)) \
                    go (fn [m] (when (%not (%int? m)) (error :m)) \
                         (if (%lt m 1) :done (go (helper m))))] \
             (begin (git helper) (go n))))",
        &mut symbols,
    );
    let info = analyze_regions_with(&hir, &arena, pc.call_classification);
    let cells = compiled_cells(&info);
    assert_eq!(
        cells.len(),
        1,
        "precondition: exactly one compiled cell — `helper`'s forward cell; got {cells:?}",
    );
    let (_helper, cell) = cells[0];
    assert!(
        !info.frame_held_regions.contains(&cell),
        "a forward cell r{} whose binding is STORED must not clear the admission — the \
         projection carries the binding's refusal as it carries its verdict; \
         frame_held={:?}",
        cell.0,
        info.frame_held_regions,
    );
}
