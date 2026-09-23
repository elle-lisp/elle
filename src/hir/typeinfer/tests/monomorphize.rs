// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/impl/hir.md
//! Container-dispatch wrapper monomorphization.
//!
//! A `(match (type-of coll) …arms…)` wrapper (`push`/`put`/`del`/`add`/…) whose
//! container argument has a statically proven concrete container type collapses,
//! at the call site, to the arm that type selects: a direct call to that arm's
//! monomorphic `%`-op. An unproven container leaves the dispatch intact.

use super::{compile_fhir, count_calls_to, infer_and_rewrite};
use crate::symbol::SymbolTable;

/// Proven concrete container ⇒ the wrapper call is rewritten to the arm's
/// monomorphic op; the multi-arm dispatch call ceases to exist.
#[test]
fn container_dispatch_wrapper_monomorphizes_on_proven_container() {
    let src = "(def dynamic-put %put) \
               (defn myp [c k v] (match (type-of c) \
                 :@struct (%put-struct-mut c k v) \
                 :struct (%put-struct c k v) \
                 _ (dynamic-put c k v))) \
               (let [s @{:x 0}] (myp s :x 9))";
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    infer_and_rewrite(&mut hir, &arena, &mut Default::default()).expect("infer");
    assert_eq!(
        count_calls_to(&hir, &arena, "myp"),
        0,
        "a proven-@struct container collapses the dispatch-wrapper call to its arm"
    );
}

/// Unproven container (joined to Top across disjoint call sites) ⇒ the dispatch
/// wrapper call survives; the pass must not over-monomorphize.
#[test]
fn container_dispatch_wrapper_stays_dynamic_on_unproven_container() {
    let src = "(def dynamic-put %put) \
               (defn myp [c k v] (match (type-of c) \
                 :@struct (%put-struct-mut c k v) \
                 :struct (%put-struct c k v) \
                 _ (dynamic-put c k v))) \
               (defn g [c] (myp c :x 9)) (g @{:x 0}) (g @[1])";
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    infer_and_rewrite(&mut hir, &arena, &mut Default::default()).expect("infer");
    assert!(
        count_calls_to(&hir, &arena, "myp") >= 1,
        "an unproven container leaves the dispatch-wrapper call intact"
    );
}
