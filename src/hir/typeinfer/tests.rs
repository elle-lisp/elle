use super::infer_and_rewrite;
use crate::hir::pattern::{HirPattern, PatternLiteral};
use crate::hir::types::{TyId, TypeInterner};
use crate::hir::{BindingArena, Hir, HirKind};
use crate::symbol::SymbolTable;

/// Compile a source file to canonical (functionalized) HIR, mirroring the
/// escape-test harness. The lib/test `Config::default` is checked-off, so
/// `infer_and_rewrite` runs (it early-returns under checked-on).
fn compile_fhir(src: &str, symbols: &mut SymbolTable) -> (Hir, BindingArena) {
    let mut cctx = crate::pipeline::CompileCtx::new();
    let (hir, arena, _names) =
        crate::pipeline::compile_file_to_fhir(src, symbols, &mut cctx, "<test>").expect("compile");
    (hir, arena)
}

/// The set of inferred node types for `src`.
fn inferred_types(src: &str) -> Vec<TyId> {
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    let info = infer_and_rewrite(&mut hir, &arena, &symbols).expect("infer");
    info.hir_types.values().copied().collect()
}

/// Compile `src` through the file front end (which runs `infer_and_rewrite`),
/// discarding the result and surfacing only success/failure. `Err` is the
/// monomorphization proof obligation firing (or any earlier compile error).
fn compile_result(src: &str) -> Result<(), String> {
    let mut symbols = SymbolTable::new();
    let mut cctx = crate::pipeline::CompileCtx::new();
    crate::pipeline::compile_file_to_fhir(src, &mut symbols, &mut cctx, "<test>").map(|_| ())
}

/// The proof obligation's gating counter-factual: a monomorphic `-mut`
/// container op applied to an *unproven* binding
/// (`c` is a bare parameter, never narrowed, so its type stays `Top`) has no
/// static guarantee it is even a container — and in silent (unchecked-
/// intrinsics) context there is no runtime guard to catch a mismatch. So the
/// silent lowering is illegal and must be a compile error. Counter-factual:
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
/// (called all over with `@array`s), which made the whole unchecked stdlib fail
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

/// The mutability axis is load-bearing: `:array` narrows to the *immutable*
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

/// Set constructors carry their declared RetType, read through `def_by_name`:
/// `(set …)` is `Set`, `(@set …)` is `MutableSet`.
#[test]
fn set_constructors_infer_their_declared_rettype() {
    assert!(inferred_types("(set 1 2)").contains(&TypeInterner::SET));
    assert!(inferred_types("(@set 1 2)").contains(&TypeInterner::MUTABLE_SET));
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

/// Monomorphic `%push-array-mut` pins its result `MutableArray` from the *op*,
/// not the input: applied to an *immutable* `[1 2]` the result is still
/// `MutableArray` (the funnel store returns arg0, which the `-mut` variant
/// asserts is mutable). Since the input is immutable `Array`, the only source of
/// `MUTABLE_ARRAY` in `hir_types` is the op's declared return type. Counter-factual
/// against a `FirstArg`/`Unknown` return that would inherit the input's immutability.
#[test]
fn push_array_mut_result_is_mutable_array_from_the_op() {
    assert!(
        inferred_types("(%push-array-mut [1 2] 3)").contains(&TypeInterner::MUTABLE_ARRAY),
        "%push-array-mut result must be MutableArray even on an immutable input"
    );
}

/// Symmetric: monomorphic `%push-array` (immutable) returns a fresh immutable
/// `Array` even when applied to a *mutable* `@[1 2]`. The input is `MutableArray`,
/// so the `ARRAY` TyId can only originate in the op's declared return type — the
/// Fresh immutable twin. Counter-factual against returning `FirstArg`.
#[test]
fn push_array_result_is_immutable_array_from_the_op() {
    assert!(
        inferred_types("(%push-array @[1 2] 3)").contains(&TypeInterner::ARRAY),
        "%push-array result must be immutable Array even on a mutable input"
    );
}

/// `%put-struct-mut` pins `MutableStruct` from the op: applied to an *immutable*
/// `{:a 1}` the only source of `MUTABLE_STRUCT` in `hir_types` is the op's return
/// type (the input is immutable `Struct`). Counter-factual against a `FirstArg`
/// return inheriting the input's immutability.
#[test]
fn put_struct_mut_result_is_mutable_struct_from_the_op() {
    assert!(
        inferred_types("(%put-struct-mut {:a 1} :b 2)").contains(&TypeInterner::MUTABLE_STRUCT),
        "%put-struct-mut result must be MutableStruct even on an immutable input"
    );
}

/// Symmetric: `%put-struct` (immutable) returns a fresh `Struct` even on a mutable
/// `@{:a 1}` input (whose own type is `MutableStruct`).
#[test]
fn put_struct_result_is_immutable_struct_from_the_op() {
    assert!(
        inferred_types("(%put-struct @{:a 1} :b 2)").contains(&TypeInterner::STRUCT),
        "%put-struct result must be immutable Struct even on a mutable input"
    );
}

/// `%put-array-mut` pins `MutableArray` from the op on an immutable `[1 2]` input.
#[test]
fn put_array_mut_result_is_mutable_array_from_the_op() {
    assert!(
        inferred_types("(%put-array-mut [1 2] 0 9)").contains(&TypeInterner::MUTABLE_ARRAY),
        "%put-array-mut result must be MutableArray even on an immutable input"
    );
}

/// `%put-array` (immutable) returns a fresh `Array` even on a mutable `@[1 2]` input.
#[test]
fn put_array_result_is_immutable_array_from_the_op() {
    assert!(
        inferred_types("(%put-array @[1 2] 0 9)").contains(&TypeInterner::ARRAY),
        "%put-array result must be immutable Array even on a mutable input"
    );
}

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
// end-to-end leak is the oracle's `each-array`/io-yield probes
// (tests/elle/oracle.lisp), which run under the full stdlib.

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

/// Constructor RetType forward-inference: `@string` always builds a fresh
/// mutable string (`RetType::MutableString`), and — now that their mutability
/// is fixed by the constructor rather than the argument (bytes.rs) —
/// `bytes`/`@bytes` declare `Bytes`/`MutableBytes`. The snippet is just the
/// constructor call, so the asserted type can only come from the declared
/// `RetType` read through `def_by_name`.
#[test]
fn constructors_infer_their_declared_rettype() {
    assert!(inferred_types("(@string)").contains(&TypeInterner::MUTABLE_STRING));
    assert!(inferred_types("(bytes 1 2)").contains(&TypeInterner::BYTES));
    assert!(inferred_types("(@bytes 1 2)").contains(&TypeInterner::MUTABLE_BYTES));
}
