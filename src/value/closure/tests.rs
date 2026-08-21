//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::hir::region::StaticRegion;

#[test]
fn template_region_table_is_static_regions_at_least_two() {
    // A2: `ClosureTemplate.region_table` carries the typed `StaticRegion`
    // slots cloned from its `LirFunction`, not the bare `u32`/`RegionId`
    // alias. Every slot is a real compile-time region slot (≥ 2); slot 1 is
    // reserved and never minted into a function/template table.
    //
    // Counterfactual: while the field was `Vec<u32>`, neither the
    // `vec![StaticRegion::new(..)]` assignment nor the `&Vec<StaticRegion>`
    // binding compiled — so this test could not be written.
    let mut t = ClosureTemplate::new(Rc::new(Vec::new()), Arity::Exact(0), Rc::new(Vec::new()));
    t.region_table = vec![StaticRegion::new(2).unwrap(), StaticRegion::new(3).unwrap()];
    let table: &Vec<StaticRegion> = &t.region_table;
    for sr in table {
        assert!(
            sr.get() >= 2,
            "template region_table slot must be >= 2 (slot 1 is reserved and \
                 never minted into a table), got {}",
            sr.get(),
        );
    }
}
