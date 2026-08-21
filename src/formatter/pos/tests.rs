//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn byte_offsets_round_trip_and_order() {
    assert_eq!(ByteOffset::new(7).get(), 7);
    assert!(ByteOffset::new(3) < ByteOffset::new(9));
    // The sentinel sorts after every real offset.
    assert!(ByteOffset::new(usize::MAX - 1) < ByteOffset::MAX);
}

#[test]
fn line_and_col_round_trip() {
    assert_eq!(LineNum::new(1).get(), 1);
    assert_eq!(ColNum::new(5).get(), 5);
    assert_eq!(LineNum::new(2), LineNum::new(2));
}

#[test]
fn blank_count_caps_at_ceiling() {
    // Spec: a 5-blank-line run collapses to the ceiling of 2.
    assert_eq!(BlankCount::new(5).capped_at(2), 2);
    // A run already within the ceiling is unchanged.
    assert_eq!(BlankCount::new(1).capped_at(2), 1);
    assert_eq!(BlankCount::new(0).capped_at(2), 0);
}
