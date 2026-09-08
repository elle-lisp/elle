// audited: 2026-09-07
// src/hir/AGENTS.md
// docs/impl/hir.md
//! Type-directed dead-arm pruning: a `(match (type-of x) …)` over a statically
//! known scrutinee keeps its live arm and loses the rest.

use super::compile_fhir;
use crate::hir::pattern::{HirPattern, PatternLiteral};
use crate::hir::{Hir, HirKind};
use crate::symbol::SymbolTable;

/// Does any `Match` arm in the tree carry the `:kw` keyword-literal pattern
/// (directly or inside an `or`-pattern)? The `each` macro's dispatch arms are
/// exactly such keyword-literal arms (`:fiber`, `(or :set :@set)`, …), so this
/// detects whether a given dispatch arm survived pruning.
fn has_keyword_arm(hir: &Hir, kw: &str) -> bool {
    fn pat_has(p: &HirPattern, kw: &str) -> bool {
        match p {
            HirPattern::Literal(PatternLiteral::Keyword(s)) => {
                s.strip_prefix(':').unwrap_or(s) == kw
            }
            HirPattern::Or(alts) => alts.iter().any(|a| pat_has(a, kw)),
            _ => false,
        }
    }
    fn walk(h: &Hir, kw: &str, found: &mut bool) {
        if let HirKind::Match { arms, .. } = &h.kind {
            if arms.iter().any(|(p, _, _)| pat_has(p, kw)) {
                *found = true;
            }
        }
        h.for_each_child(|c| walk(c, kw, found));
    }
    let mut found = false;
    walk(hir, kw, &mut found);
    found
}

// The pruning *mechanism* is pinned here on hand-written, primitive-only
// `(match (type-of x) …)` forms (the bare test harness carries no stdlib, so the
// `each` macro — which uses stdlib `pair?` — cannot compile here). The `each`
// end-to-end shape is the oracle's `each-array` probe (tests/elle/oracle.lisp),
// which runs under the full stdlib.

/// Type-directed dead-arm pruning. A `(match (type-of a) …)` whose scrutinee `a`
/// is a literal array has every off-array arm provably unreachable, so `prune.rs`
/// removes them before region inference — otherwise `a` is referenced (and its
/// release point computed) inside a dead arm, leaking its region (the
/// `each`-over-collection over-keep the `each` macro otherwise hits per op, pinned
/// end-to-end by the oracle's `each-array` probe). The
/// off-type arms (`:fiber`, `:set`, `:struct`) must be gone; the
/// live `:array` arm kept.
#[test]
fn typeof_match_prunes_dead_arms_for_a_literal_array_scrutinee() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir(
        "(let [a [1 2 3]] \
           (match (type-of a) \
             :array (length a) \
             :fiber (fiber/resume a) \
             (or :set :@set) (->array a) \
             :struct (pairs a) \
             _ nil))",
        &mut symbols,
    );
    assert!(
        !has_keyword_arm(&hir, "fiber"),
        "the :fiber arm is unreachable for a literal array and must be pruned"
    );
    assert!(
        !has_keyword_arm(&hir, "set"),
        "the :set arm is unreachable for a literal array and must be pruned"
    );
    assert!(
        !has_keyword_arm(&hir, "struct"),
        "the :struct arm is unreachable for a literal array and must be pruned"
    );
    assert!(
        has_keyword_arm(&hir, "array"),
        "the :array arm is the live dispatch arm and must be kept"
    );
}

/// The dispatch also narrows through an alias to a primitive whose return type
/// is concrete: `(->array …)` declares `RetType::Array`, so a binding initialized
/// from it is statically `:array` and the off-array arms prune. This is the
/// shape the async scheduler hits — `io/wait` declares `RetType::Array`, so
/// `(each c in (io/wait …) …)` prunes the same way.
#[test]
fn typeof_match_prunes_through_a_primitive_rettype_alias() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir(
        "(let [a (->array (list 1 2 3))] \
           (match (type-of a) \
             :array (length a) \
             :fiber (fiber/resume a) \
             _ nil))",
        &mut symbols,
    );
    assert!(
        !has_keyword_arm(&hir, "fiber"),
        "->array declares RetType::Array, so the :fiber arm must be pruned"
    );
    assert!(
        has_keyword_arm(&hir, "array"),
        "the live :array arm must be kept"
    );
}

/// Soundness boundary — under-pruning is the safe direction. When the
/// scrutinee's concrete type is NOT statically known (a bare parameter, whose
/// runtime type varies by call site), NO arm is pruned: every dispatch arm
/// survives so a value of any runtime type reaches its correct arm. Over-pruning
/// a live arm would be a use-after-free (its release computed as if dead) or a
/// wrong/`:match-error` dispatch.
#[test]
fn typeof_match_keeps_all_arms_for_an_unknown_scrutinee() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir(
        "(defn f [c] \
           (match (type-of c) \
             :array (length c) \
             :fiber (fiber/resume c) \
             (or :set :@set) (->array c) \
             _ nil)) \
         (f [1 2 3])",
        &mut symbols,
    );
    assert!(
        has_keyword_arm(&hir, "fiber"),
        "c's concrete type is not statically known, so no arm may be pruned"
    );
    assert!(
        has_keyword_arm(&hir, "set"),
        "c's concrete type is not statically known, so no arm may be pruned"
    );
}

/// A user binding that shadows a primitive constructor must NOT be read as that
/// primitive's `RetType`: `(def array …)` here returns a fiber, not an array, so
/// the scrutinee's type is unknown and no arm prunes. The gate is `is_primitive`
/// (set only by `bind_primitives`), so a same-named user binding is excluded —
/// reading its name's `RetType` would be an unsound prune (→ a UAF on the live
/// `:fiber` path, which is the arm actually taken at runtime here).
#[test]
fn typeof_match_does_not_prune_through_a_user_shadowed_constructor() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir(
        "(def array (fn [& xs] (fiber/new (fn [] 1) 1))) \
         (let [a (array 1 2 3)] \
           (match (type-of a) \
             :array (length a) \
             :fiber (fiber/resume a) \
             _ nil))",
        &mut symbols,
    );
    assert!(
        has_keyword_arm(&hir, "fiber"),
        "a user-shadowed `array` is not the primitive; its RetType must not be \
         trusted, so the :fiber arm (the live one at runtime) must be kept"
    );
}
