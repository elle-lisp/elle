//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn set_and_is_set_beyond_64() {
    // The whole point: a slot at index ≥ 64 is representable and precise,
    // where the old u64 mask could not name it at all.
    let mut m = CaptureMask::empty();
    m.set(3);
    m.set(70);
    m.set(129);
    assert!(m.is_set(3));
    assert!(m.is_set(70));
    assert!(m.is_set(129));
    assert!(!m.is_set(2));
    assert!(!m.is_set(64));
    assert!(!m.is_set(128));
    assert!(!m.is_set(500));
    assert!(!m.is_empty());
}

#[test]
fn empty_and_from_u64() {
    assert!(CaptureMask::empty().is_empty());
    assert!(CaptureMask::from_u64(0).is_empty());
    let m = CaptureMask::from_u64(0b1010);
    assert!(m.is_set(1));
    assert!(m.is_set(3));
    assert!(!m.is_set(0));
    assert_eq!(m.low_u64(), 0b1010);
}

#[test]
fn equal_sets_compare_equal_regardless_of_trailing_zeros() {
    // PartialEq is derived and `Closure` equality leans on it, so two
    // encodings of the same set must be `==`.
    let mut a = CaptureMask::empty();
    a.set(5);
    let b = CaptureMask::from_words(vec![1 << 5, 0, 0]);
    assert_eq!(a, b);
}

#[test]
fn roundtrips_through_words() {
    let mut m = CaptureMask::empty();
    m.set(0);
    m.set(64);
    m.set(65);
    let rt = CaptureMask::from_words(m.words().to_vec());
    assert_eq!(m, rt);
}
