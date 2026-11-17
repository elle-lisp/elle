//! audited: 2026-11-17
//! A completion's birthplace, which builds an answer in one region and hands it over once, and the submission id.
//!
//! docs/impl/io/inflight.md

use super::*;

/// Every value one completion builds shares one region
/// (docs/impl/io/inflight.md).
///
/// The counter-factual: a birthplace that minted per allocation reads correct
/// on every value and leaks per OBJECT rather than per completion, since only
/// one of those regions can be the one the answer is handed over on. The signal
/// arrives as a residue that scales with how many objects an answer is made of
/// — a struct per signal event, a string per address — which no per-call rate
/// separates from the per-call defect.
#[test]
fn a_birthplace_coins_one_region_for_every_value_it_builds() {
    let h = crate::primitives::ctx::TestHeap::new();
    let mut birth = Birthplace::on(h.heap());
    let first = birth.alloc().string("first");
    let second = birth.alloc().string("second");
    let region_of = |v| crate::value::arena::region_of(h.heap(), v);
    let (first_region, second_region) = (region_of(first), region_of(second));
    birth.hand_over();
    assert_eq!(
        first_region, second_region,
        "a completion's answer is one region, whatever it is assembled from"
    );
    assert!(first_region.is_some(), "a built value lives in a region");
}

/// The birth reference goes exactly once, and a birthplace that built nothing
/// owes nothing.
///
/// The trap: `hand_over` runs after the struct that takes the value has
/// recorded its own edge, so the region here is held by nobody and the release
/// frees it. That is what makes the live-region count the reading — a count on
/// the region alone would report the same number whether the reference went or
/// stayed.
#[test]
fn a_birthplace_hands_its_region_over_once() {
    let h = crate::primitives::ctx::TestHeap::new();
    let mut birth = Birthplace::on(h.heap());
    let _answer = birth.alloc().string("answer");
    let coined = h.heap().active_region_count();
    birth.hand_over();
    assert_eq!(
        h.heap().active_region_count(),
        coined - 1,
        "the region a completion built its answer in outlived the handover"
    );
    birth.hand_over();
    assert_eq!(
        h.heap().active_region_count(),
        coined - 1,
        "a second handover released a reference the first already gave up"
    );

    let mut empty = Birthplace::on(h.heap());
    let none = h.heap().active_region_count();
    empty.hand_over();
    assert_eq!(
        h.heap().active_region_count(),
        none,
        "a completion that built nothing released something"
    );
}

/// An ask that builds nothing gives its id back and releases nothing.
///
/// The trap: `alloc` mints on the ask, and the empty list is an immediate, so
/// an empty listing asks and builds nothing. A release of that id names a
/// region that never materialized: a phantom-region panic under debug
/// assertions, and an over-free counted without them. The probe mint puts a
/// known id on top of the free list, so the birthplace mints that id, and the
/// next mint shows whether the handover gave it back.
#[test]
fn a_birthplace_that_built_nothing_gives_its_id_back() {
    let h = crate::primitives::ctx::TestHeap::new();
    let probe = h.heap().new_runtime_region_tracked();
    let id = probe.region();
    h.heap().recycle_unmaterialized_region(probe);
    let over_frees = h.heap().over_frees();
    let mut birth = Birthplace::on(h.heap());
    let answer = birth.alloc().list(std::iter::empty());
    assert_eq!(answer, Value::EMPTY_LIST, "the empty list is an immediate");
    birth.hand_over();
    assert_eq!(
        h.heap().over_frees(),
        over_frees,
        "the handover released a region nothing built"
    );
    assert_eq!(
        h.heap().new_runtime_region(),
        id,
        "the handover kept an id nothing materialized"
    );
}

/// A completion let go without saying what becomes of its region fails the
/// build.
///
/// The counter-factual: with no `Drop` on the birthplace, this same code
/// strands one region per operation and every other assertion in this file
/// still reads correct. The residue shows up only in a whole-process census,
/// which cannot name the operation that left it.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "still holding the region it built its answer in")]
fn a_birthplace_dropped_holding_its_region_says_so() {
    let h = crate::primitives::ctx::TestHeap::new();
    let mut birth = Birthplace::on(h.heap());
    let _answer = birth.alloc().string("answer");
    drop(birth);
}

/// `into_result` gives the birth reference away with the value: the region
/// stays live for whoever took it.
///
/// The trap: a body that releases — the way `into_value` does — reads correct
/// on the value itself, because nothing has reused those pages yet. The live
/// count is what separates the two, and the caller this exists for reclaims no
/// region while it runs, so a release here is a use-after-free it would reach
/// much later.
#[test]
fn a_completion_read_straight_out_keeps_what_it_built() {
    let h = crate::primitives::ctx::TestHeap::new();
    let mut birth = Birthplace::on(h.heap());
    let answer = birth.alloc().string("answer");
    let built = h.heap().active_region_count();
    let result = Completion::ok(SubmissionId::from_raw(1), birth, answer).into_result();
    assert!(result.is_ok(), "a completion built ok answered as an error");
    assert_eq!(
        h.heap().active_region_count(),
        built,
        "the region the value lives in was released under the reader that took it"
    );
}

/// Discarding a queue releases what every completion in it built, not just the
/// one that was read.
///
/// The counter-factual: reading the front and dropping the rest empties the
/// queue just as well, and every test that asserts on a completion's result
/// still passes. What it leaves is a region per unread completion — the state a
/// backend torn down holding a full queue is in.
#[test]
fn a_discarded_queue_releases_what_every_completion_built() {
    let h = crate::primitives::ctx::TestHeap::new();
    let empty = h.heap().active_region_count();
    let queue: Vec<Completion> = (1..=3)
        .map(|n| {
            let mut birth = Birthplace::on(h.heap());
            let answer = birth.alloc().string("answer");
            Completion::ok(SubmissionId::from_raw(n), birth, answer)
        })
        .collect();
    assert_eq!(
        h.heap().active_region_count(),
        empty + 3,
        "each completion builds its answer in a region of its own"
    );
    Completion::discard_all(queue);
    assert_eq!(
        h.heap().active_region_count(),
        empty,
        "a discarded queue left behind what its completions built"
    );
}

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
