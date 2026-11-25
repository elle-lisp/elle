// audited: 2026-11-25
//! Pins the branch-arm window's live-in premise: a region anchors on the sites
//! that mint it, never on a binding an arm binds to it.
//!
//! docs/impl/region/window.md

use super::*;

#[test]
fn a_holder_an_arm_binds_through_a_nested_let_leaves_the_param_live_in() {
    // `s` is no bare alias: its init is a `let`. It still allocates nothing, so
    // the release of `xs` routes through the prologue's slot, and the region the
    // branch received is the branch's to release. Anchoring on `s`'s definition
    // would keep the release in the ELSE arm and strand `xs` on the THEN arm.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (i xs) (if (%eq i 0) (length xs) (let [s (let [u xs] u)] (length s))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a nested-let holder in one arm must not keep the param's release there"
    );
}

#[test]
fn a_holder_whose_init_may_build_leaves_the_param_live_in() {
    // The `each` shape: the holder's init hands back either the param or a value
    // it builds. The built value is born in the arm, and the param is not, so the
    // param's release still moves to the merge.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (i xs) (if (%eq i 0) (length xs) \
           (let [s (if (%eq i 1) xs (%pair xs nil))] (length s))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a holder whose init may build must not keep the param's release in its arm"
    );
}

#[test]
fn a_holder_bound_to_a_begin_leaves_the_param_live_in() {
    // The init's last form names the param after a read. Nothing is born.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (i xs) (if (%eq i 0) (length xs) (let [s (begin (length xs) xs)] (length s))))",
    );
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &info, "xs", &[then_id, else_id]),
        "a begin-bound holder in one arm must not keep the param's release there"
    );
}

#[test]
fn a_collection_an_arm_destructures_keeps_its_release_in_the_arm() {
    // `mutated::a_value_allocated_in_an_arm_keeps_its_in_arm_release` pins the
    // allocation's birth site. A rest pattern over an array BUILDS a fresh collection at the `Destructure`,
    // which no `alloc_region` entry names. The pattern node is its birth site, so
    // the collection keeps its release inside the arm that built it.
    let (hir, _arena, _symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) 0 (let [[a & r] xs] (length r))))");
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let built: Vec<Region> = info
        .pattern_rest_regions
        .values()
        .flatten()
        .map(|c| c.region)
        .collect();
    assert!(!built.is_empty(), "the rest pattern builds a collection");
    assert!(
        built
            .iter()
            .all(|&r| !region_release_clears_the_arms(&hir, &info, r, &[else_id])),
        "a collection built in an arm must keep its release inside that arm; {built:?}"
    );
}
