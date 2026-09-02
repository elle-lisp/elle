use super::infer_and_rewrite;
use crate::hir::pattern::{HirPattern, PatternLiteral};
use crate::hir::types::{TyId, TypeInterner};
use crate::hir::{BindingArena, Hir, HirKind};
use crate::symbol::SymbolTable;

/// Compile a source file to canonical (functionalized) HIR, mirroring the
/// escape-test harness. `infer_and_rewrite` runs unconditionally on every
/// compile (driven from `hir::regularize`).
fn compile_fhir(src: &str, symbols: &mut SymbolTable) -> (Hir, BindingArena) {
    let mut cctx = crate::pipeline::CompileCtx::new();
    let (hir, arena) =
        crate::pipeline::compile_file_to_fhir(src, symbols, &mut cctx, "<test>").expect("compile");
    (hir, arena)
}

/// The set of inferred node types for `src`.
fn inferred_types(src: &str) -> Vec<TyId> {
    let mut symbols = SymbolTable::new();
    let (mut hir, arena) = compile_fhir(src, &mut symbols);
    let info = infer_and_rewrite(&mut hir, &arena, &mut Default::default()).expect("infer");
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

// ═══ The prove-or-reject contract (docs/intrinsics.md § The contract) ═══
//
// A `%`-intrinsic in call position compiles iff the operands' inferred types
// discharge the op's full soundness contract; provably-wrong AND unprovable
// operands alike are compile errors. Value position stays the registered
// NativeFn. One row per contract family below: the reject direction (the
// counter-factual) and the discharge direction (the over-rejection guard).

/// Does the tree contain an `Intrinsic` node for the `%`-op named `name`?
/// Distinguishes opcode lowering (an `Intrinsic` node) from native-funnel
/// lowering (a `Call` to the registered NativeFn) in routing tests.
fn has_intrinsic_named(hir: &Hir, name: &str) -> bool {
    let mut found = false;
    fn walk(h: &Hir, name: &str, found: &mut bool) {
        if let HirKind::Intrinsic { op, .. } = &h.kind {
            if op.name() == name {
                *found = true;
            }
        }
        h.for_each_child(|c| walk(c, name, found));
    }
    walk(hir, name, &mut found);
    found
}

/// Does the tree contain a `Call` whose callee is a `Var` binding named `name`?
fn has_call_to(hir: &Hir, arena: &BindingArena, name: &str) -> bool {
    let mut found = false;
    fn walk(h: &Hir, arena: &BindingArena, name: &str, found: &mut bool) {
        if let HirKind::Call { func, .. } = &h.kind {
            if let HirKind::Var(b) = &func.kind {
                if arena.get(*b).name == crate::value::SymbolId::of(name) {
                    *found = true;
                }
            }
        }
        h.for_each_child(|c| walk(c, arena, name, found));
    }
    walk(hir, arena, name, &mut found);
    found
}

/// Reject, provably wrong: a string operand is never a Number, so `(%add "a" 3)`
/// cannot lower soundly and must be a compile error naming the op.
#[test]
fn provably_wrong_intrinsic_operand_is_a_compile_error() {
    let err = compile_result("(%add \"a\" 3)")
        .expect_err("a string operand to %add must be rejected at compile time");
    assert!(err.contains("%add"), "error must name the op; got: {err}");
}

/// Reject, unprovable: bare parameters have no inferred type, so `(%add a b)`
/// has no proof and no runtime guard — it must be a compile error, not a
/// silent-garbage lowering.
#[test]
fn unprovable_intrinsic_operands_are_a_compile_error() {
    let err = compile_result("(defn f [a b] (%add a b))")
        .expect_err("unproven operands to %add must be rejected at compile time");
    assert!(err.contains("%add"), "error must name the op; got: {err}");
}

/// Discharge: literal operands prove themselves.
#[test]
fn proven_literal_intrinsic_operands_compile() {
    compile_result("(%add 1 2)").expect("proven literal ints discharge %add");
    compile_result("(%lt 1 2)").expect("proven literal ints discharge %lt");
    compile_result("(%shl 4 1)").expect("proven literal ints discharge %shl");
}

/// Discharge: the total ops carry no contract — equality, identity, truthiness
/// negation, pair construction, and the predicates are total on every value,
/// so unproven operands are fine. Counter-factual against an over-strict
/// contract (e.g. Bool for `%not`, whose opcode is truthiness negation).
#[test]
fn contract_free_total_ops_compile_on_unknown_operands() {
    compile_result("(defn f [x y] (%eq x y))").expect("%eq is total");
    compile_result("(defn f [x y] (%ne x y))").expect("%ne is total");
    compile_result("(defn f [x y] (%identical? x y))").expect("%identical? is total");
    compile_result("(defn f [x] (%not x))").expect("%not is truthiness negation, total");
    compile_result("(defn f [x y] (%pair x y))").expect("%pair is total");
    compile_result("(defn f [x] (%type-of x))").expect("%type-of is total");
    compile_result("(defn f [x] (%int? x))").expect("predicates are total");
}

/// Reject: a container op on an unproven binding. The polymorphic ops carry
/// the same family obligation as the monomorphic variants — there is no
/// runtime dispatch in an opcode.
#[test]
fn unproven_container_operand_is_a_compile_error() {
    let err = compile_result("(defn f [c] (%length c))")
        .expect_err("unproven container to %length must be rejected");
    assert!(
        err.contains("%length"),
        "error must name the op; got: {err}"
    );
    let err = compile_result("(defn f [c] (%get c 0))")
        .expect_err("unproven container to %get must be rejected");
    assert!(err.contains("%get"), "error must name the op; got: {err}");
    let err = compile_result("(defn f [c] (%put c :k 1))")
        .expect_err("unproven container to %put must be rejected");
    assert!(err.contains("%put"), "error must name the op; got: {err}");
}

/// Reject: `%first`/`%rest` trust their operand to be a pair.
#[test]
fn unproven_pair_operand_is_a_compile_error() {
    let err = compile_result("(defn f [p] (%first p))")
        .expect_err("unproven pair to %first must be rejected");
    assert!(err.contains("%first"), "error must name the op; got: {err}");
}

/// Discharge: an `if` type-guard proves the operand in the then-branch —
/// the intrinsic-predicate spelling.
#[test]
fn if_guard_narrowed_operand_discharges() {
    compile_result("(defn f [x] (if (%int? x) (%add x 1) 0))")
        .expect("the %int? guard proves x in the then-branch");
}

/// Discharge: a diverging guard proves the fall-through — the stdlib wrapper
/// shape. After `(when (%not (%int? b)) (emit :error …))` every path on which
/// `b` is not an int has diverged, so `b` is an int in the code that follows.
#[test]
fn diverging_guard_narrows_the_fall_through() {
    compile_result(
        "(defn f [b] \
           (when (%not (%int? b)) (emit :error {:message \"f: int required\"})) \
           (%add b 1))",
    )
    .expect("the diverging guard proves b on the fall-through path");
}

/// Reject, div family: the divisor must be provably nonzero — a type proof
/// alone is not the full soundness contract for `%div`/`%rem`/`%mod`, whose
/// opcode is silent and total only for a nonzero divisor.
#[test]
fn div_by_provably_zero_divisor_is_a_compile_error() {
    let err = compile_result("(%div 1 0)")
        .expect_err("a literal zero divisor must be rejected at compile time");
    assert!(err.contains("%div"), "error must name the op; got: {err}");
}

/// Reject, div family: operands proven Number but the divisor's nonzero-ness
/// unproven is still a reject — the value obligation is part of the contract.
#[test]
fn div_with_type_proven_but_zero_unproven_divisor_is_a_compile_error() {
    let err = compile_result("(defn f [x y] (if (%int? x) (if (%int? y) (%div x y) 0) 0))")
        .expect_err("an int divisor not proven nonzero must be rejected");
    assert!(err.contains("%div"), "error must name the op; got: {err}");
}

/// Discharge, div family: a nonzero literal divisor.
#[test]
fn div_with_nonzero_literal_divisor_compiles() {
    compile_result("(defn f [x] (if (%int? x) (%div x 2) 0))")
        .expect("a nonzero literal divisor discharges the value obligation");
}

/// Discharge, div family: a diverging zero guard proves the divisor nonzero
/// on the fall-through — the `/`·`rem`·`mod` wrapper shape.
#[test]
fn div_with_zero_guarded_divisor_compiles() {
    compile_result(
        "(defn f [x y] \
           (when (%not (%int? x)) (emit :error {:message \"f: int required\"})) \
           (when (%not (%int? y)) (emit :error {:message \"f: int required\"})) \
           (when (%eq y 0) (emit :error {:message \"f: zero divisor\"})) \
           (%div x y))",
    )
    .expect("the diverging zero guard proves the divisor nonzero");
}

// ═══ Lowering routes (docs/intrinsics.md § Lowering) ═══

/// A proven non-storing op lowers to the opcode `Intrinsic` node.
#[test]
fn non_storing_intrinsic_routes_to_the_opcode_node() {
    let mut symbols = SymbolTable::new();
    let (hir, _arena) = compile_fhir("(%add 1 2)", &mut symbols);
    assert!(
        has_intrinsic_named(&hir, "%add"),
        "a proven %add lowers to the opcode Intrinsic node"
    );
}

/// A proven storing op lowers to the native funnel `Call` — the escape-correct
/// path whose region accounting records cross-region edges — never to an
/// inline opcode.
#[test]
fn storing_intrinsic_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1]] (%array-push a 2))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%array-push"),
        "%array-push must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%array-push"),
        "%array-push lowers to a Call to its registered NativeFn"
    );
}

/// `%pop` rides the native funnel so its moved-out element carries the
/// call-result region accounting.
#[test]
fn pop_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1 2]] (%pop a))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%pop"),
        "%pop must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%pop"),
        "%pop lowers to a Call to its registered NativeFn"
    );
}

/// `%freeze`/`%thaw` are copying constructors on the native side of the split.
#[test]
fn freeze_routes_to_the_native_funnel_call() {
    let mut symbols = SymbolTable::new();
    let (hir, arena) = compile_fhir("(let [a @[1]] (%freeze a))", &mut symbols);
    assert!(
        !has_intrinsic_named(&hir, "%freeze"),
        "%freeze must not lower to an inline opcode"
    );
    assert!(
        has_call_to(&hir, &arena, "%freeze"),
        "%freeze lowers to a Call to its registered NativeFn"
    );
}

/// The whole-stdlib discharge proof: primitives + core.lisp + prelude +
/// stdlib.lisp all compile under the always-on proof obligation, and the
/// wrapper surface works. Every `%`-site in the stdlib is discharged by a
/// guard the narrowing reads — this test is the canonical reference that the
/// discharge holds.
#[test]
fn full_stdlib_discharges_the_intrinsic_proof_obligations() {
    let mut rt = crate::runtime::Runtime::new();
    let (vm, symbols, cctx) = rt.parts();
    let v = crate::pipeline::eval_all("(+ 1 2)", symbols, vm, cctx, "<test>")
        .expect("the stdlib boots and the wrapper surface evaluates");
    assert_eq!(v.as_int(), Some(3));
}

/// Reject: a struct-family `%get` key must be proven hashable — the surface
/// `get` raises :type-error for an unhashable key (a float can never BE a
/// struct key), and the opcode's unreachable-by-proof path is a loud panic,
/// so an unproven or provably-unhashable key cannot lower silently.
#[test]
fn unhashable_struct_get_key_is_a_compile_error() {
    let err = compile_result("(%get {:a 1} 1.5)")
        .expect_err("a float struct key must be rejected at compile time");
    assert!(err.contains("%get"), "error must name the op; got: {err}");
    let err = compile_result("(defn f [k] (%get {:a 1} k))")
        .expect_err("an unproven struct key must be rejected at compile time");
    assert!(err.contains("%get"), "error must name the op; got: {err}");
    compile_result("(%get {:a 1} :a)").expect("a proven keyword key discharges");
}

/// Call-site argument forwarding is a complete proof only for a function used
/// EXCLUSIVELY in callee position — then every call is syntactically visible
/// and the parameter join enumerates them. One value-position use (stored,
/// passed to a HOF, returned, exported) means invisible callers exist, so the
/// join must not discharge the function's raw %-sites: the author writes the
/// guard (or declares `(numeric!)`).
#[test]
fn value_position_use_disables_param_join_proofs() {
    // Callee-only: the (f 3) join proves x.
    compile_result("(defn f [x] (%add x 1)) (f 3)")
        .expect("callee-only use keeps the call-site join proof");
    // Same function, but also passed as a VALUE: the join no longer counts.
    let err = compile_result("(defn f [x] (%add x 1)) (f 3) (def g f) (g 4)")
        .expect_err("a value-position use makes callers invisible; the join must not prove");
    assert!(err.contains("%add"), "error must name the op; got: {err}");
    // The guarded form compiles regardless of how the function travels.
    compile_result("(defn f [x] (if (%int? x) (%add x 1) 0)) (f 3) (def g f) (g 4)")
        .expect("a guard discharges independently of call-site visibility");
}

/// A parameter join must enumerate EVERY visible call site's argument type —
/// including "unknown". One caller passing an untyped value makes the
/// parameter unprovable, so the body's %-site rejects until the author
/// guards; typed callers alone must never prove it.
#[test]
fn unknown_typed_caller_defeats_param_join_proofs() {
    compile_result("(defn opaque [] (first (list 1)))\n(defn f [x] (%add x 1)) (f 3) (f (opaque))")
        .expect_err("an unknown-typed call site must make the param unprovable")
        .contains("%add")
        .then_some(())
        .expect("error must name %add");
    compile_result("(defn f [x] (%add x 1)) (f 3) (f 4)")
        .expect("all-typed call sites still prove the param");
    compile_result(
        "(defn opaque [] (first (list 1)))\n\
         (defn f [x] (if (%int? x) (%add x 1) 0)) (f 3) (f (opaque))",
    )
    .expect("the guard discharges regardless of caller types");
}

// ── Container-dispatch wrapper monomorphization (F1b) ─────────────────────────
//
// A `(match (type-of coll) …arms…)` wrapper (`push`/`put`/`del`/`add`/…) whose
// container argument has a statically-proven concrete container type collapses,
// at the call site, to the arm that type selects — a direct call to that arm's
// monomorphic `%`-op. This removes the multi-arm dispatch and, with it, the
// container-scrutinee over-keep the dispatch strands in the textually-last arm
// (the F1b leak: the owned container arg's release lands in an arm the executed
// path never reaches). An UNPROVEN container leaves the dispatch intact.

/// Count Call nodes whose callee resolves to a binding named `name`.
fn count_calls_to(hir: &Hir, arena: &BindingArena, name: &str) -> usize {
    let mut n = 0;
    if let HirKind::Call { func, .. } = &hir.kind {
        if let Some(b) = super::unwrap_callee_binding(func) {
            if arena.get(b).name == crate::value::SymbolId::of(name) {
                n += 1;
            }
        }
    }
    hir.for_each_child(|c| n += count_calls_to(c, arena, name));
    n
}

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
