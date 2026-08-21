//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn submission_id_round_trips_through_raw() {
    for raw in [0u64, 1, 42, u64::MAX] {
        assert_eq!(SubmissionId::from_raw(raw).as_u64(), raw);
    }
}

#[test]
fn submission_id_orders_by_underlying_value() {
    // The scheduler relies on later submissions comparing greater than
    // earlier ones (see the *monotonic_ids backend tests).
    assert!(SubmissionId::from_raw(1) < SubmissionId::from_raw(2));
    assert_eq!(SubmissionId::from_raw(7), SubmissionId::from_raw(7));
    assert_ne!(SubmissionId::from_raw(7), SubmissionId::from_raw(8));
}

#[test]
fn submission_id_displays_as_its_integer() {
    // io/submit hands the raw integer back to Lisp; Display must match.
    assert_eq!(format!("{}", SubmissionId::from_raw(99)), "99");
}
