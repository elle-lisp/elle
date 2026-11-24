// audited: 2026-11-24
//! Pins the head route for a region the branch's result carries past the join: a use it refuses, and one release per path.
//!
//! docs/impl/region/compensate.md

use super::*;

#[test]
fn a_use_before_the_branch_consumed_past_it_takes_no_head_release() {
    // The over-free counter-factual: `k` is `g`'s first argument, read before
    // the branch and consumed by the call past it. Read by its position alone,
    // the use is no later use, and the else arm would release `k` before `g`
    // takes it. The `let` gives the branch a result binding, which a bare
    // argument does not get.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (c) (begin (def x (f 1)) (def k (fn () (length x))) \
           (g k (let [r (if c k (fn () 0))] (r)))))",
    );
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !arm_compensates(&hir, &arena, &info, "k", else_id),
        "a region a call past the join consumes takes no head release"
    );
}

#[test]
fn a_cond_releases_once_per_path() {
    // `cond` gives one arm set per level, and the first level's rest arm holds
    // both later bodies. The counter-factual: read level by level, the rest arm
    // and each body inside it take a head release, so a path runs two.
    let (hir, arena, _symbols, info) = analyze_with_class(
        "(fn (c) (begin (def x (f 1)) (def k (fn () (length x))) \
           (def r (cond (%eq c 0) k (%eq c 1) (fn () 1) (fn () 2))) (r)))",
    );
    let b = find_binding_by_name(&hir, "k", &arena).expect("the local `k`");
    let regions = &info.binding_source_regions[&b];
    let heads = info
        .branch_compensation
        .values()
        .filter(|rs| regions.iter().any(|r| rs.contains(r)))
        .count();
    assert_eq!(
        heads, 1,
        "one head release of `k` per path through the cond"
    );
}
