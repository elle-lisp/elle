// audited: 2026-11-20
//! A stdlib type predicate dispatches on `%type-of`, so a guard written with one grafts to no call.
//!
//! src/hir/AGENTS.md
//! docs/impl/graft.md

use super::super::{compiled, count_calls_to, count_intrinsics};

/// Nothing proves `x`, so the grafted `number?` stays a dispatch on its type.
/// The dispatch is the `%type-of` intrinsic, which is no call and so ends no
/// fact. The counter-factual is the call to `type-of` each predicate made.
#[test]
fn a_grafted_type_predicate_makes_no_call() {
    let src = "(defn h [x] (number? x)) (h 1) (h :k)";
    let (hir, arena) = compiled(src);
    assert_eq!(
        count_calls_to(&hir, &arena, "number?"),
        0,
        "{src}: number? grafts"
    );
    assert_eq!(
        count_calls_to(&hir, &arena, "type-of"),
        0,
        "{src}: no call to type-of"
    );
    assert!(
        count_intrinsics(&hir, "%type-of") > 0,
        "{src}: the dispatch reads %type-of"
    );
}
