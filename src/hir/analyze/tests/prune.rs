// audited: 2026-11-09
//! A merge nothing reads leaves no merge var, and the tree it would have had without the merge.
//!
//! docs/bindings.md

use super::nested::{analyze_file, body_of, nodes};
use super::*;

/// Whether any node under `hir` assigns a binding.
fn assigns(hir: &Hir) -> bool {
    nodes(hir)
        .iter()
        .any(|n| matches!(n.kind, HirKind::Assign { .. }))
}

// Counter-factual: a merge frame always set its merge var, so a merge var that
// nothing read after the form kept the value of each def alive, and every
// `each` over a list leaked the list.
#[test]
fn a_merge_nothing_reads_leaves_no_merge_var() {
    let result = analyze_file("(defn f [c] (when c (def x 1)) 0)");
    assert!(
        !assigns(body_of(&result, "f")),
        "nothing reads x after the when, so no merge var is set"
    );
    let result = analyze_file("(defn f [c] (when c (def x 1)) x)");
    assert!(
        assigns(body_of(&result, "f")),
        "the read after the when keeps its merge var"
    );
}

// Counter-factual: the prune unwrapped a merge assign into a read and left a
// one-node `Begin` around the branch, and type inference then lost the guard
// a `%`-intrinsic in the branch relied on.
#[test]
fn a_pruned_merge_leaves_the_tree_it_would_have_had() {
    let result = analyze_file("(defn f [c] (if c (begin (def x 1) x) 0))");
    let HirKind::If { then_branch, .. } = &body_of(&result, "f").kind else {
        panic!("f's body is its if, with no begin around it");
    };
    let HirKind::Begin(exprs) = &then_branch.kind else {
        panic!("the branch is its begin, got {:?}", then_branch.kind);
    };
    assert!(
        matches!(exprs[0].kind, HirKind::Define { .. }),
        "the def is a plain Define, got {:?}",
        exprs[0].kind
    );
}
