// audited: 2026-11-24
//! Pins last use through a branch's test: the test is its own last use, and a name the test reads still reaches the arms.
//!
//! docs/impl/region/lastuse.md
//!
//! The counter-factual: the branch's node takes the test's value. For a
//! functionalized `while` that node is the `if` the loop recurs from, and a
//! release there runs on the exit path alone, so every pass that loops on
//! strands what its test made (tests/impl/region-loop-test-release.lisp).

use super::*;

/// True when `id` names `h` or a node below it.
fn holds(h: &super::Hir, id: HirId) -> bool {
    if h.id == id {
        return true;
    }
    let mut found = false;
    h.for_each_child(|c| found = found || holds(c, id));
    found
}

/// The innermost branch test whose subtree holds `id`: the test of an `if`, of a
/// `cond` clause or of a `while`, with the branch's own node beside it.
fn test_holding(h: &super::Hir, id: HirId) -> Option<(HirId, HirId)> {
    let mut tests: Vec<&super::Hir> = Vec::new();
    match &h.kind {
        HirKind::If { cond, .. } | HirKind::While { cond, .. } => tests.push(cond),
        HirKind::Cond { clauses, .. } => tests.extend(clauses.iter().map(|(t, _)| t)),
        _ => {}
    }
    for t in tests {
        if holds(t, id) {
            return test_holding(t, id).or(Some((t.id, h.id)));
        }
    }
    let mut found = None;
    h.for_each_child(|c| found = found.or_else(|| test_holding(c, id)));
    found
}

/// Assert that each `(string …)` allocation in `source` that sits in a branch's
/// test takes a last use inside that test.
fn assert_tests_own_their_values(source: &str, expected: usize) {
    let (hir, arena, symbols, info) = analyze_with_hir(source);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let allocs = find_calls_to_primitive(&hir, "string", &arena, &symbols);
    assert_eq!(
        allocs.len(),
        expected,
        "the (string …) allocations of {source}"
    );
    for alloc in allocs {
        let (test, branch) =
            test_holding(&hir, alloc).expect("each allocation sits in a branch's test");
        let got = info
            .last_use
            .get(&alloc)
            .copied()
            .expect("missing last_use");
        let low = order.low(&test).unwrap_or(0);
        assert!(
            ord(got) >= low && ord(got) <= ord(test),
            "the allocation @{} in the test @{} of the branch @{} takes last use @{} \
             (ord {}), outside the test's interval [{low}, {}] — a release past the \
             branch runs once for a loop's every pass, in {source}",
            alloc.0,
            test.0,
            branch.0,
            got.0,
            ord(got),
            ord(test),
        );
    }
}

#[test]
fn an_if_test_is_its_own_last_use() {
    assert_tests_own_their_values("(fn (n) (if (string \"a\" n) 1 2))", 1);
}

#[test]
fn a_while_test_is_its_own_last_use() {
    // The body assigns `k`, so functionalization turns the `while` into a loop
    // around an `if` whose then arm recurs.
    assert_tests_own_their_values(
        "(fn (n) (def @k 0) \
           (while (and (%lt k n) (string \"a\" k)) (assign k (%add k 1))) k)",
        1,
    );
}

#[test]
fn a_value_a_test_hands_on_is_its_test_s_too() {
    // An `if` inside the test hands either arm's value on to the test.
    assert_tests_own_their_values(
        "(fn (n) (def @k 0) \
           (while (if (%lt k n) (string \"a\" k) nil) (assign k (%add k 1))) k)",
        1,
    );
}

#[test]
fn a_cond_clause_test_is_its_own_last_use() {
    assert_tests_own_their_values("(fn (n) (cond (string \"a\" n) 1 (string \"b\" n) 2 3))", 2);
}

#[test]
fn a_name_the_test_reads_reaches_the_arm_that_reads_it() {
    // The test reads `s`, and the arm reads it again. The binding chain carries
    // the init past the arm's read, whatever the test itself takes.
    let (hir, arena, symbols, info) =
        analyze_with_hir("(fn (n) (let [s (string \"a\" n)] (if s (f s) 0)))");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let alloc = find_calls_to_primitive(&hir, "string", &arena, &symbols)[0];
    let reads = find_vars_by_name(&hir, "s", &arena, &symbols);
    assert_eq!(reads.len(), 2, "the test and the arm each read `s`");
    let arm_read = *reads.iter().max_by_key(|id| ord(**id)).unwrap();
    let got = info
        .last_use
        .get(&alloc)
        .copied()
        .expect("missing last_use");
    assert!(
        ord(got) > ord(arm_read),
        "`s`'s init takes last use @{} at or before the arm's read @{}",
        got.0,
        arm_read.0,
    );
}
