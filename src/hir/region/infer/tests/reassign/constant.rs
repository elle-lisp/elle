// audited: 2026-11-12
//! A reassigned cell whose init is a loop constant the hoist moved counts that init.
//!
//! docs/impl/hoist.md
//! docs/impl/region/bindings.md
//!
//! The counter-factual: a hoisted binding the holder index skips leaves the
//! cell as the constant's sole holder, so the cell takes it by donation. Its
//! first overwrite then frees the one value every later pass starts from.

use super::*;

/// The string literal `"x"` is the init of a `var` that an inner loop
/// reassigns, on every pass of an outer loop. The hoist binds `"x"` once,
/// before the outer loop, so the binding it makes holds the value the cell
/// starts from. The cell must count its init: a retain at its binder, and no
/// suppressed release of the literal's region.
#[test]
fn a_cell_counts_an_init_the_hoist_moved() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (var s \"x\")\n\
                           (var j 0)\n\
                           (while (%lt j n)\n\
                             (begin (assign s (%pair j s))\n\
                                    (assign j (%add j 1))))\n\
                           (assign i (%add i 1))))\n\
                  0)))\n\
         (h 3)",
    );
    assert!(
        !loop_builds_a_string(&hir),
        "precondition: the hoist moved the literal out of every loop"
    );
    let (s, sites) = heap_carrying_reassign(&hir, &info);
    let x = string_literal_region(&hir, &info, "x");
    assert!(
        info.binding_source_regions[&s].contains(&x),
        "precondition: the cell's init is the literal's value; regions={:?}",
        info.binding_source_regions[&s],
    );
    assert!(
        sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the cell keeps the container model"
    );
    assert!(
        !info.suppressed_decref_regions.contains(&x),
        "the hoisted binding holds the literal, so its release stays"
    );
    assert_eq!(
        info.counted_cell_init_sites.len(),
        1,
        "the cell counts its init at its binder (got {:?})",
        info.counted_cell_init_sites,
    );
}

/// Whether a string literal sits at or below some loop node of `hir`.
fn loop_builds_a_string(hir: &Hir) -> bool {
    fn has_string(h: &Hir) -> bool {
        let mut found = matches!(h.kind, HirKind::String(_));
        h.for_each_child(|c| found |= has_string(c));
        found
    }
    if matches!(hir.kind, HirKind::Loop { .. }) {
        return has_string(hir);
    }
    let mut found = false;
    hir.for_each_child(|c| found |= loop_builds_a_string(c));
    found
}
