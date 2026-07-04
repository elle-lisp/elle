//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn runtime_region_retires_ids_below_two() {
    // Ids 0 and 1 are reserved and not valid `RuntimeRegion`s; minting
    // starts at 2.
    assert_eq!(RuntimeRegion::new(0), None);
    assert_eq!(RuntimeRegion::new(1), None);
    assert_eq!(RuntimeRegion::new(2).map(RuntimeRegion::get), Some(2));
}
