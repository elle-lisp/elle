// audited: 2026-11-13
//! Each release a call's move would reorder or double, refused one at a time over the shape that is offered.
//!
//! docs/impl/region/handoff.md
//!
//! No shape in either suite reaches most of these refusals, so each test edits
//! the analysis at the `make_lowerer_with` seam. It marks `t`'s region the way
//! the refusal reads it, over a shape whose unedited analysis offers `t`, and
//! asserts the offer is gone.

use super::*;
use crate::hir::region::{Region, RegionInfo};

/// A source whose function `w` makes one call, which offers each of its
/// `args` arguments.
struct Offered {
    source: &'static str,
    args: usize,
}

/// `t` is read nowhere after `(q t)`, so the call offers it.
const ONE: Offered = Offered {
    source: "(begin (def p (fn (g) nil)) \
             (def w (fn (q xs) (let [t (xs 0)] (begin (q t) 0)))) \
             (w p [1]))",
    args: 1,
};

/// `t` and `u` are both read nowhere after `(q t u)`, so the call offers both.
const TWO: Offered = Offered {
    source: "(begin (def p (fn (a b) nil)) \
             (def w (fn (q xs) (let [t (xs 0) u (xs 1)] (begin (q t u) 0)))) \
             (w p [1 2]))",
    args: 2,
};

/// `t` is offered as in [`ONE`], and `v`'s pair is an allocation `w` releases
/// by its static slot.
const MERGED: Offered = Offered {
    source: "(begin (def p (fn (g) nil)) \
             (def w (fn (q xs) (let [t (xs 0) v (%pair xs 1)] (begin (q t) (length v))))) \
             (w p [1]))",
    args: 1,
};

/// The region of each `let` binding's init in `hir` that a call or a
/// collection literal allocates, in source order: `t`'s first.
fn let_init_regions(info: &RegionInfo, hir: &Hir) -> Vec<Region> {
    fn walk(info: &RegionInfo, h: &Hir, out: &mut Vec<Region>) {
        if let HirKind::Let { bindings, .. } = &h.kind {
            for (_, init) in bindings {
                if let (HirKind::Call { .. } | HirKind::Intrinsic { .. }, Some(&r)) =
                    (&init.kind, info.alloc_region.get(&init.id))
                {
                    out.push(r);
                }
            }
        }
        h.for_each_child(|c| walk(info, c, out));
    }
    let mut out = Vec::new();
    walk(info, hir, &mut out);
    out
}

/// The offers `w` in `shape` makes once `refuse` has edited the analysis,
/// handed the regions [`let_init_regions`] finds. The unedited analysis must
/// offer every argument, or the edit proves nothing.
fn offers_once(shape: Offered, refuse: impl FnOnce(&mut RegionInfo, &[Region])) -> OfferList {
    let unedited = offers_in(named(&compile_to_lir(shape.source), "w"));
    assert_eq!(
        unedited.len(),
        shape.args,
        "the unedited shape no longer offers every argument (offers={unedited:?})",
    );
    let (mut lowerer, hir) = make_lowerer_with(shape.source, |info, hir| {
        let regions = let_init_regions(info, hir);
        refuse(info, &regions);
    });
    let module = lowerer.lower(&hir).expect("lower");
    offers_in(named(&module, "w"))
}

/// Assert that `t`, position 0, is not offered once `refuse` has run.
fn assert_refused(what: &str, refuse: impl FnOnce(&mut RegionInfo, &[Region])) {
    let offers = offers_once(ONE, refuse);
    assert!(
        offers.iter().all(|(position, _)| *position != 0),
        "{what} was offered (offers={offers:?})",
    );
}

#[test]
fn a_region_released_by_its_static_slot_is_not_offered() {
    // Outside `call_result_regions` a release names the region's static slot,
    // `DecrefRegion(rid)`, and loads no slot. The stamp leaves it armed, so the
    // callee's release of the moved reference would be the second.
    assert_refused("a region released by id", |info, regions| {
        for r in regions {
            info.call_result_regions.remove(r);
        }
    });
}

#[test]
fn a_merged_child_is_not_offered() {
    // A merge child has no release of its own: its merged root's frees it. A
    // move would hand the callee a reference the frame never releases here.
    // The root is `v`'s pair, which `w` allocates and releases by its slot.
    let offers = offers_once(MERGED, |info, regions| {
        info.merged_parent.insert(regions[0], regions[1]);
    });
    assert!(
        offers.is_empty(),
        "a merge child was offered (offers={offers:?})"
    );
}

#[test]
fn a_suppressed_release_is_not_offered() {
    // A reassigned top-level binding's store path releases this value, and the
    // call node emits nothing for it.
    assert_refused("a suppressed release", |info, regions| {
        info.suppressed_decref_regions.insert(regions[0]);
    });
}

#[test]
fn a_co_owned_group_member_is_not_offered() {
    // The group's one `FreeRegionGroup` frees every member together.
    assert_refused("a co-owned group member", |info, regions| {
        info.owned_group_members.insert(regions[0]);
    });
}

#[test]
fn a_mutated_bindings_value_is_not_offered() {
    // A mutated binding's slot holds whatever was last assigned to it by the
    // release, not the value the region names.
    assert_refused("a mutated binding's value", |info, regions| {
        info.mutated_binding_value_regions.insert(regions[0]);
    });
}

#[test]
fn a_cell_release_is_not_offered() {
    // A capture cell's release frees the box, never the value inside it.
    assert_refused("a cell release", |info, regions| {
        info.cell_release_regions.insert(regions[0]);
    });
}

#[test]
fn a_value_a_counted_cell_stores_is_offered() {
    // The contrast to the cell release: a 1-slot container takes a counted
    // reference of its own when it stores the value, and the frame's release
    // still loads the producer's slot. The move hands the callee the frame's
    // reference and leaves the cell's alone.
    let offers = offers_once(ONE, |info, regions| {
        info.cell_stored_regions.insert(regions[0]);
    });
    assert_eq!(
        offers.len(),
        1,
        "a value a counted cell stores is not offered (offers={offers:?})",
    );
}

#[test]
fn a_transfer_adopt_is_not_offered() {
    // The release is an `AdoptIntoActivation`, which consumes the region's
    // whole count, not one reference.
    assert_refused("a transfer adopt", |info, regions| {
        info.transfer_adopt_regions.insert(regions[0]);
    });
}

#[test]
fn an_adopt_owner_is_not_offered() {
    // An owner's release drops the subtree its members live in, and each
    // member's own release must run first.
    let member = Region(u32::MAX - 1);
    assert_refused("an adopt owner", |info, regions| {
        let site = info.alloc_region.keys().next().copied().expect("a site");
        info.owned_adopt_edges
            .insert(site, vec![(member, regions[0])]);
    });
}

#[test]
fn a_release_ordered_against_another_at_the_call_is_not_offered() {
    // `u` reads out of `t`, so the call node orders `u`'s release ahead of
    // `t`'s. Moved, `t`'s release runs inside the callee, ahead of `u`'s.
    let offers = offers_once(TWO, |info, regions| {
        let site = info.alloc_region.keys().next().copied().expect("a site");
        info.counted_read_aliases
            .push((site, regions[1], regions[0]));
    });
    assert!(
        offers.is_empty(),
        "a release the call orders against another was offered (offers={offers:?})",
    );
}

#[test]
fn a_reassigned_binding_is_not_offered() {
    // A reassigned binding's slot is not a release route: by the release it
    // holds whatever was last assigned (docs/impl/region/bindings.md), and the
    // stamp a taken move leaves there would zero a live value.
    //
    // The trap: functionalization turns an `assign` in straight-line code into
    // a fresh immutable binding, which owns its slot and may be offered, and a
    // loop's reassigned binding reached no call bucket in any shape tried. So
    // the test marks every `let` binding reassigned at the `make_lowerer_with`
    // seam, over the shape [`ONE`]. Without the refusal, `t` is offered here
    // as that shape offers it.
    fn let_bindings(h: &Hir, out: &mut Vec<Binding>) {
        if let HirKind::Let { bindings, .. } = &h.kind {
            out.extend(bindings.iter().map(|(b, _)| *b));
        }
        h.for_each_child(|c| let_bindings(c, out));
    }
    let (mut lowerer, hir) = make_lowerer_with(ONE.source, |info, hir| {
        let mut lets = Vec::new();
        let_bindings(hir, &mut lets);
        info.reassigned_local_bindings.extend(lets);
    });
    let module = lowerer.lower(&hir).expect("lower");
    let offers = offers_in(named(&module, "w"));
    assert!(
        offers.is_empty(),
        "a reassigned binding was offered (offers={offers:?})",
    );
}
