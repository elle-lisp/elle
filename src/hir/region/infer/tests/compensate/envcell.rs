// audited: 2026-09-28
//! Pins the env cell's compensating release: both routes name the box, so no refusal
//! about the holder's value withdraws them.
//!
//! docs/impl/region/compensate.md

use super::*;

// ── The env cell's compensating release ───────────────────────────────
//
// docs/impl/region/compensate.md. An env cell's box is minted once per activation and
// released through `LoadCaptureRaw` + `DecrefCellRegion`, which names the box
// rather than the holder's slot. Every refusal that would decline a per-arm release
// of it — a reassigned holder, a capturer's alias, the return frontier, the
// same-node retain — is a claim about the VALUE that holder names, so both routes
// carry the box: `head` where the arm names the binding nowhere, `tail` after that
// arm's last use where it reads it.

/// The env-cell region of the binding named `name` — the `cell_release_regions`
/// member among its source regions.
fn env_cell_region(hir: &Hir, arena: &BindingArena, info: &RegionInfo, name: &str) -> Region {
    let b =
        find_binding_by_name(hir, name, arena).unwrap_or_else(|| panic!("no binding named {name}"));
    info.binding_source_regions
        .get(&b)
        .into_iter()
        .flatten()
        .copied()
        .find(|r| info.cell_release_regions.contains(r))
        .unwrap_or_else(|| {
            panic!(
                "binding `{name}` must hold an env-cell region; source={:?} cell_release={:?}",
                info.binding_source_regions.get(&b),
                info.cell_release_regions,
            )
        })
}

#[test]
fn a_falling_through_arm_compensates_the_env_cell_its_sibling_relocated() {
    // `(if t (g) 0)` — `g` captures the in-lambda mutable `c`, so the box is an env
    // cell whose one `DecrefCellRegion` sits in the THEN arm (the frame-exit
    // relocation moves it ahead of that arm's `TailCall`). The ELSE arm reaches the
    // merge and names `c` nowhere, so it is a dead sibling arm and owes the head
    // release; without it the box strands once per call.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (n t) (def @c n) (let [g (fn () c)] (if t (g) 0)))");
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cell = env_cell_region(&hir, &arena, &info, "c");
    assert!(
        info.branch_compensation
            .get(&else_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "the falling-through arm must release the env cell r{}; branch_compensation={:?}",
        cell.0,
        info.branch_compensation,
    );
}

#[test]
fn a_reassigned_holder_does_not_withdraw_its_env_cell_compensation() {
    // The mutated face. A reassigned holder poisons a release routed through its
    // SLOT, and this release names the box no `assign` repoints — so the same head
    // release is owed. Pins that the refusal is read per region, not per holder.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (n t) (def @c n) (let [g (fn () (assign c (%add c 1)) c)] (if t (g) 0)))",
    );
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cell = env_cell_region(&hir, &arena, &info, "c");
    assert!(
        info.branch_compensation
            .get(&else_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "a reassigned holder's env cell r{} must still take the head release; \
         branch_compensation={:?}",
        cell.0,
        info.branch_compensation,
    );
}

#[test]
fn an_env_cell_takes_the_tail_route_on_the_arm_that_reads_it() {
    // `(if t c (g))` — the capture-use of `c` resolves through `g`'s last use, so
    // the box's `decref_point` follows the CALL and lands in the else arm. The then
    // arm READS `c`, making it a used sibling arm: the head route fires before the
    // arm's body and would free the box under that read. The `tail` route releases
    // after the read instead, and needs no same-node retain — the box's holders are
    // the frame's env slot plus one counted `closure ⊇ cell` edge per capturer, and
    // no use of the binding yields the box.
    let (hir, arena, _symbols, info) =
        analyze_with_class("(fn (n t) (def @c n) (let [g (fn () c)] (if t c (g))))");
    let cell = env_cell_region(&hir, &arena, &info, "c");
    assert!(
        info.branch_arm_decrefs
            .values()
            .any(|rs| rs.contains(&cell)),
        "the arm that reads the cell's binding must release the box r{} after that \
         read; branch_arm_decrefs={:?}",
        cell.0,
        info.branch_arm_decrefs,
    );
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !info
            .branch_compensation
            .get(&then_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "the reading arm must take no head release of r{} — that frees the box \
         under its own read",
        cell.0,
    );
}
