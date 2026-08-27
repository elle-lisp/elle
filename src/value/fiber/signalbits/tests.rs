//! Pins the set semantics of `SignalBits`.
//!
//! The predicate names are the point of these tests. `intersects` shares a
//! bit; it is not a subset test, and reading it as one would silently change
//! how every fiber mask catches a compound signal.

use super::SignalBits;
use crate::signals::{SIG_ERROR, SIG_EXEC, SIG_IO, SIG_OK, SIG_YIELD};

/// One bit each, distinct, well clear of the compiler-reserved range.
const A: SignalBits = SignalBits::from_bit(32);
const B: SignalBits = SignalBits::from_bit(33);
const C: SignalBits = SignalBits::from_bit(34);

#[test]
fn intersects_is_shared_bits_not_subset() {
    // A partial overlap intersects. A subset test would reject both of these,
    // because neither side contains the other.
    assert!((A | B).intersects(B | C));
    assert!((B | C).intersects(A | B));

    // Disjoint sets do not.
    assert!(!A.intersects(B));
    assert!(!(A | B).intersects(C));
}

#[test]
fn intersects_is_symmetric() {
    // Symmetry is what separates an intersection from a containment: a subset
    // test holds in one direction only.
    assert!(A.intersects(A | B));
    assert!((A | B).intersects(A));
}

#[test]
fn the_empty_set_intersects_nothing() {
    // Not even itself: there is no shared bit to share.
    assert!(!SignalBits::EMPTY.intersects(SignalBits::EMPTY));
    assert!(!SignalBits::EMPTY.intersects(SignalBits::ALL));
    assert!(!SignalBits::ALL.intersects(SignalBits::EMPTY));
}

#[test]
fn all_intersects_every_nonempty_set() {
    assert!(SignalBits::ALL.intersects(A));
    assert!(SignalBits::ALL.intersects(SIG_ERROR | SIG_YIELD));
}

#[test]
fn the_empty_set_has_one_spelling() {
    // `SIG_OK` is the signal-domain name for the empty set, not a second set.
    assert_eq!(SIG_OK, SignalBits::EMPTY);
    assert!(SIG_OK.is_empty());
    assert!(!SIG_ERROR.is_empty());
}

#[test]
fn has_bit_agrees_with_intersects_on_a_single_bit() {
    assert!((A | B).has_bit(32));
    assert!(!(A | B).has_bit(34));
    assert_eq!((A | B).has_bit(32), (A | B).intersects(A));
    assert_eq!((A | B).has_bit(34), (A | B).intersects(C));
}

#[test]
fn covers_privileges_no_bit_over_another() {
    // A subprocess request is |:io :exec|. Both bits route it, and a mask
    // naming either one catches it — `:io` is not a precondition for `:exec`
    // taking effect (#895, tests/elle/mask-exec-routes.lisp).
    let request = SIG_IO | SIG_EXEC;
    assert!(SIG_EXEC.covers(request));
    assert!(SIG_IO.covers(request));
    assert!((SIG_ERROR | SIG_EXEC).covers(request));
    assert!(SignalBits::ALL.covers(request));
    // A mask sharing no bit with the request still catches nothing.
    assert!(!SIG_ERROR.covers(request));
}

#[test]
fn a_yield_mask_does_not_catch_an_io_request() {
    // The guarantee the old SIG_IO special case bought: an intermediate fiber
    // masking |:yield| must not swallow a request the scheduler has to service.
    // It now holds because the two genuinely share no bit — an I/O request
    // raises |:io| and does not carry :yield.
    assert!(!SIG_YIELD.intersects(SIG_IO));
    assert!(!SIG_YIELD.covers(SIG_IO));
    assert!(!SIG_YIELD.covers(SIG_IO | SIG_EXEC));
}

#[test]
fn covers_is_intersects_plus_the_empty_signal() {
    assert!(SIG_YIELD.covers(SIG_YIELD));
    assert!(!SIG_YIELD.covers(SIG_ERROR));
    // A mask of |:log| catches the compound |:log :audit| on the shared bit.
    assert!(A.covers(A | B));
    assert!(B.covers(A | B));
    assert!(!A.covers(B));
}

#[test]
fn every_mask_covers_a_normal_return() {
    assert!(SIG_OK.covers(SIG_OK));
    assert!(SIG_YIELD.covers(SIG_OK));
    assert!(SignalBits::EMPTY.covers(SIG_OK));
}

#[test]
fn set_algebra() {
    assert_eq!(A.union(B), A | B);
    assert_eq!((A | B).intersection(B | C), B);
    assert_eq!((A | B).subtract(B), A);
    assert_eq!(A.complement(), !A);
    assert_eq!(SignalBits::EMPTY.complement(), SignalBits::ALL);
}

#[test]
fn trailing_zeros_names_the_lowest_bit() {
    assert_eq!(A.trailing_zeros(), 32);
    assert_eq!((B | C).trailing_zeros(), 33);
}
