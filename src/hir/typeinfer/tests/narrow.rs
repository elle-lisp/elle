// audited: 2026-09-07
// src/hir/AGENTS.md
// docs/impl/hir.md
//! What the inference proves about a binding: dispatch-arm narrowing, the
//! let-aliased scrutinee, and the `or`/`and` operand join.

use super::{compile_result, inferred_types};
use crate::hir::types::TypeInterner;

/// The proof obligation's gating counter-factual: a monomorphic `-mut`
/// container op applied to an *unproven* binding
/// (`c` is a bare parameter, never narrowed, so its type stays `Top`) has no
/// static guarantee it is even a container — and a lowered call-position
/// intrinsic carries no runtime guard to catch a mismatch. Prove-or-reject:
/// an unproven operand makes the silent lowering illegal, so it must be a
/// compile error. Counter-factual:
/// before the op-site consult this lowered silently (compiled clean).
#[test]
fn silent_unproven_monomorphic_op_is_compile_error() {
    let err = compile_result("(defn f [c] (%push-array-mut c 3))")
        .expect_err("an unproven container must be rejected in silent context");
    assert!(
        err.contains("%push-array-mut"),
        "the error must name the offending op; got: {err}"
    );
}

/// The "proven ⇒ no error" direction (over-rejection guard): the `:@array`
/// arm of `(match (type-of c) …)` narrows `c` to `MutableArray`, discharging
/// the obligation, so the silent `%push-array-mut` is legal and compiles. This
/// is the spec's motivating shape — the gate must admit it.
///
/// Multi-arm routing-shape discharge (the real stdlib `push`/`put` shape, all
/// six monomorphic ops) is covered behaviourally — compile *and* run, on every
/// tier — by the corpus test `tests/elle/monoroute.lisp`, which exercises the
/// actual stdlib routing rather than a transcribed copy of it.
#[test]
fn proven_monomorphic_op_compiles_under_match_narrowing() {
    compile_result("(defn f [c] (match (type-of c) :@array (%push-array-mut c 3) _ nil))")
        .expect("a container the :@array arm proves mutable-array discharges the obligation");
}

/// A `(match (type-of c) :KW …)` arm is **authoritative** within its body —
/// the runtime `type-of` dispatch guarantees `c`'s concrete type there, so the
/// narrowing must *override* the binding's inferred type, not `meet` with it.
///
/// The counterfactual: when `c` is a parameter the inference has already widened
/// to a *disjoint* concrete type (here `MUTABLE_ARRAY`, forwarded from the
/// `(f @[1])` caller), `meet(MUTABLE_ARRAY, ARRAY) = BOTTOM` — so a `meet`-based
/// narrowing leaves the `:array → %push-array` arm's container unproven and the
/// silent monomorphic op a compile error, *even though that arm only runs when
/// `c` is an immutable array*. This is exactly the stdlib `push`/`put` shape
/// (called all over with `@array`s), which made the whole stdlib fail
/// to compile at `<stdlib>:477` — the regression this pins. Override discharges
/// it; the arm narrows `c` to `ARRAY` regardless of the wider accumulated type.
#[test]
fn match_typeof_arm_narrows_authoritatively_over_a_called_param() {
    compile_result(
        "(defn f [c] (match (type-of c) \
           :array (%push-array c 3) \
           :@array (%push-array-mut c 3) \
           _ nil)) \
         (f @[1])",
    )
    .expect(
        "the :array arm proves c immutable-array regardless of the MUTABLE_ARRAY \
         the (f @[1]) caller forwarded — the arm's type-of dispatch is authoritative",
    );
}

/// Gating counter-factual for the monomorphization proof obligation: the
/// `:@array` arm of a `(match (type-of c) …)` must prove `c : MutableArray` so
/// a monomorphic `%push-array-mut` it routes to is a statically-typed, silent
/// call. The source constructs no array, so the *only* way MutableArray
/// reaches `hir_types` is `infer_node`'s `Match` arm narrowing `c` in that arm.
#[test]
fn match_typeof_narrows_at_array_arm_to_mutable_array() {
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :@array c _ nil))")
            .contains(&TypeInterner::MUTABLE_ARRAY)
    );
}

/// The mutability axis decides the route: `:array` narrows to the *immutable*
/// `Array`, never `MutableArray` — else `push` could not route `:array` →
/// `%push-array` (Fresh) distinctly from `:@array` → `%push-array-mut`
/// (funnel). Counter-factual against a map collapsing the two mutabilities.
#[test]
fn match_typeof_immutable_array_arm_narrows_to_immutable_not_mutable() {
    let tys = inferred_types("(defn f [c] (match (type-of c) :array c _ nil))");
    assert!(
        tys.contains(&TypeInterner::ARRAY),
        "narrows to immutable Array"
    );
    assert!(
        !tys.contains(&TypeInterner::MUTABLE_ARRAY),
        "must NOT collapse :array onto MutableArray"
    );
}

/// Struct arms, both mutabilities.
#[test]
fn match_typeof_narrows_struct_arms() {
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :@struct c _ nil))")
            .contains(&TypeInterner::MUTABLE_STRUCT)
    );
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :struct c _ nil))")
            .contains(&TypeInterner::STRUCT)
    );
}

/// String arms — exercises the new MutableString TyId.
#[test]
fn match_typeof_narrows_string_arms() {
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :@string c _ nil))")
            .contains(&TypeInterner::MUTABLE_STRING)
    );
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :string c _ nil))")
            .contains(&TypeInterner::STRING)
    );
}

/// Bytes arms — exercises the new MutableBytes TyId.
#[test]
fn match_typeof_narrows_bytes_arms() {
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :@bytes c _ nil))")
            .contains(&TypeInterner::MUTABLE_BYTES)
    );
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :bytes c _ nil))")
            .contains(&TypeInterner::BYTES)
    );
}

/// Set arms — exercises the new Set/MutableSet TyIds (the previously-deferred
/// "set has no TyId" row).
#[test]
fn match_typeof_narrows_set_arms() {
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :@set c _ nil))")
            .contains(&TypeInterner::MUTABLE_SET)
    );
    assert!(
        inferred_types("(defn f [c] (match (type-of c) :set c _ nil))")
            .contains(&TypeInterner::SET)
    );
}

/// Over-narrowing guard: a non-type keyword arm proves nothing, so no
/// container type appears. Counter-factual against narrowing on *any*
/// keyword-literal arm rather than only the recognized container keywords.
#[test]
fn match_typeof_non_type_keyword_arm_narrows_nothing() {
    let tys = inferred_types("(defn f [c] (match (type-of c) :foo c _ nil))");
    let container = [
        TypeInterner::ARRAY,
        TypeInterner::MUTABLE_ARRAY,
        TypeInterner::STRUCT,
        TypeInterner::MUTABLE_STRUCT,
        TypeInterner::STRING,
        TypeInterner::MUTABLE_STRING,
        TypeInterner::BYTES,
        TypeInterner::MUTABLE_BYTES,
        TypeInterner::SET,
        TypeInterner::MUTABLE_SET,
    ];
    assert!(
        !tys.iter().any(|t| container.contains(t)),
        "a :foo arm proves no container type; narrowing must not fire"
    );
}

/// The `(let [ta (type-of c)] (match ta …))` idiom must narrow `c` exactly as
/// the inline `(match (type-of c) …)` does: the scrutinee `ta` is an immutable
/// alias of `(type-of c)`, so `typeof_subject_binding` resolves it back to `c`.
/// The source constructs no array, so the only source of `MUTABLE_ARRAY` in
/// `hir_types` is the `:@array` arm narrowing the resolved subject `c`.
/// Counter-factual: without the alias resolution the scrutinee is an opaque
/// `Var(ta)`, nothing narrows, and `MUTABLE_ARRAY` never appears.
#[test]
fn match_typeof_let_aliased_scrutinee_narrows() {
    assert!(
        inferred_types("(defn f [c] (let [ta (type-of c)] (match ta :@array c _ nil)))")
            .contains(&TypeInterner::MUTABLE_ARRAY)
    );
}

/// The motivating shape end-to-end: a let-aliased `(type-of c)` dispatch must
/// discharge the monomorphic `%push-array-mut` obligation in its `:@array` arm,
/// just like the inline form (`proven_monomorphic_op_compiles_under_match_narrowing`).
/// Counter-factual: before alias resolution this rejected — the arm proved
/// nothing about `c`, so the silent op was an unprovable-operand compile error.
#[test]
fn let_aliased_typeof_match_discharges_monomorphic_op() {
    compile_result(
        "(defn f [c] (let [ta (type-of c)] \
           (match ta :@array (%push-array-mut c 3) _ nil)))",
    )
    .expect("the let-aliased :@array arm proves c mutable-array and discharges the op");
}

/// Soundness invariant: the alias resolution narrows the subject only while the
/// subject still holds the value `(type-of …)` measured. A *mutable* subject that
/// is reassigned between the alias binding and the match no longer does, so the
/// arm must not narrow it — here `c` is reassigned to an int, so the silent
/// `%push-array-mut c` stays an unprovable-operand error (`inferred: int`). The
/// `collect_typeof_aliases` subject-mutation gate enforces this by construction;
/// this pins the end-to-end guarantee so a future change to either the gate or
/// the cell-narrowing path cannot silently narrow a stale type onto a live cell.
#[test]
fn match_typeof_let_alias_declines_when_subject_reassigned() {
    compile_result(
        "(defn f [] \
           (def @c @[1 2]) \
           (let [ta (type-of c)] \
             (assign c 5) \
             (match ta :@array (%push-array-mut c 3) _ nil)))",
    )
    .expect_err("a reassigned mutable subject must not inherit the stale type-of narrowing");
}

/// `(or a b)` returns one of its operands (the first truthy, else the last), so
/// its result type is the JOIN of the operand types — exactly as `if` joins its
/// branches. When both operands are proven `Number`, the `or` is `Number` and can
/// feed a silent `%add`. Counter-factual: while `or` typed to Top, this rejected
/// with `operand 1 … not a proven number`. (`(or a b)` here uses two distinct
/// proven-int calls so no constant-fold/dedup can collapse the `or` first.)
#[test]
fn or_result_type_is_the_join_of_its_operands() {
    compile_result(
        "(defn g [x] (when (%not (%int? x)) (error :e)) x) \
         (defn f [x] (when (%not (%int? x)) (error :e)) \
           (%add (or (g x) (g (%add x 1))) 3))",
    )
    .expect("both or-operands prove Number, so the or is Number and %add is silent");
}

/// `(and a b)` likewise returns one of its operands (the first falsy, else the
/// last), so its type is the join of the operands. Two proven-`Number` operands
/// make the `and` `Number`. Counter-factual: rejected while `and` typed to Top.
#[test]
fn and_result_type_is_the_join_of_its_operands() {
    compile_result(
        "(defn g [x] (when (%not (%int? x)) (error :e)) x) \
         (defn f [x] (when (%not (%int? x)) (error :e)) \
           (%add (and (g x) (g (%add x 1))) 3))",
    )
    .expect("both and-operands prove Number, so the and is Number and %add is silent");
}

/// Soundness of the join: it is over ALL operands, not one. `(or (g x) :kw)` types
/// as `Number ⊔ Keyword` — the join conservatively admits the keyword branch (it
/// does not reason that an int operand is always truthy and the keyword therefore
/// dead), so it does not discharge `%add` and the site must reject. Counter-factual
/// for a mistaken "type of the first (or last) operand only" rule: typing this `or`
/// as its first operand alone would wrongly prove `Number` and compile — unsound in
/// general, since a falsy first operand returns the second.
#[test]
fn heterogeneous_or_does_not_prove_a_numeric_operand() {
    compile_result(
        "(defn g [x] (when (%not (%int? x)) (error :e)) x) \
         (defn f [x] (when (%not (%int? x)) (error :e)) \
           (%add (or (g x) :kw) 3))",
    )
    .expect_err("or joins all operands, so Number ⊔ Keyword does not prove a number");
}
