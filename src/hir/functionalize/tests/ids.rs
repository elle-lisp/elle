// audited: 2026-11-24
//! Every node functionalize leaves in the tree carries a `HirId` of its own, a phi's test and scrutinee included.
//!
//! docs/impl/hir.md
//!
//! The counter-factual: one test node shared by an `if` and the phi that merges
//! after it. Its post-order index is the later visit's, past the `if`'s arms,
//! while the lowerer emits its releases at the earlier one, before the branch.
//! A release the test's last use places there frees what an arm still reads
//! (tests/impl/region-loop-test-release-uaf.lisp).

use crate::hir::expr::{Hir, HirId};
use crate::symbol::SymbolTable;

/// Every `HirId` that appears at more than one place in `hir`.
fn repeated_ids(hir: &Hir) -> Vec<HirId> {
    fn walk(h: &Hir, seen: &mut std::collections::HashSet<HirId>, out: &mut Vec<HirId>) {
        if !seen.insert(h.id) {
            out.push(h.id);
        }
        h.for_each_child(|c| walk(c, seen, out));
    }
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    walk(hir, &mut seen, &mut out);
    out
}

/// Assert that the functionalized tree of `source` repeats no `HirId`.
fn assert_ids_unique(source: &str) {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    cctx.set_graft(crate::config::GraftPolicy::Off);
    let (hir, _arena) =
        crate::pipeline::compile_file_to_fhir(source, &mut symbols, &mut cctx, "<test>")
            .expect("compile");
    let repeated = repeated_ids(&hir);
    assert!(
        repeated.is_empty(),
        "functionalize left {} node(s) at more than one place: {repeated:?}, in {source}",
        repeated.len(),
    );
}

#[test]
fn an_if_phi_tests_a_node_of_its_own() {
    assert_ids_unique("(defn f [c] (def @x 0) (if c (assign x 1) nil) x)");
}

#[test]
fn two_if_phis_test_nodes_of_their_own() {
    assert_ids_unique(
        "(defn f [c] (def @x 0) (def @y 0) \
           (if c (begin (assign x 1) (assign y 2)) nil) [x y])",
    );
}

#[test]
fn a_match_phi_matches_a_node_of_its_own() {
    assert_ids_unique("(defn f [v] (def @x 0) (match v 1 (assign x 1) _ (assign x 2)) x)");
}

#[test]
fn a_match_phi_guard_is_a_node_of_its_own() {
    assert_ids_unique("(defn f [v] (def @x 0) (match v n when (= n 1) (assign x n) _ nil) x)");
}

#[test]
fn a_cond_phi_tests_nodes_of_its_own() {
    assert_ids_unique(
        "(defn f [a b] (def @x 0) (cond a (assign x 1) b (assign x 2) (assign x 3)) x)",
    );
}
