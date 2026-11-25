// audited: 2026-11-23
// The case count a property test names reaches proptest unchanged, and
// proptest alone puts PROPTEST_CASES over it.
//
// tests/AGENTS.md

/// The counter-factual: a helper that read `PROPTEST_CASES` answers the
/// variable's count instead of the one the test named. It reads the same as
/// this helper wherever the variable is unset, and differs in every CI job
/// that sets it.
#[test]
fn the_helper_hands_proptest_the_count_it_was_given() {
    for n in [1, 37, 1000] {
        assert_eq!(crate::common::proptest_cases(n).cases, n);
    }
}
