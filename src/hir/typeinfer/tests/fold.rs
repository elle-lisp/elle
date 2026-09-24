// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/impl/graft.md
//! What the fold deletes after inference: a guard or a dispatch arm the recorded
//! types decide, and the statements after a guard certain to raise.

use super::{compile_booted, count_calls_to, count_emits, count_ops};
use crate::hir::{BindingArena, Hir, HirKind};

fn compiled(src: &str) -> (Hir, BindingArena) {
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"))
}

/// A user-written container dispatch, the shape of the stdlib's `put`.
const MYP: &str = "(def dynamic-put %put) \
                   (defn myp [c k v] (match (type-of c) \
                     :@struct (%put-struct-mut c k v) \
                     :struct (%put-struct c k v) \
                     _ (dynamic-put c k v)))";

/// The guard over a proven operand decides false, so the `if` becomes its
/// empty side and the `emit` goes with it.
#[test]
fn a_guard_over_a_proven_operand_is_deleted() {
    let src = "(defn g [b] (when (%not (%int? b)) (error :x)) (%mul 2 b)) (g 5)";
    let (hir, _) = compiled(src);
    assert_eq!(count_emits(&hir), 0, "{src}: the guard is gone");
}

/// Every caller of `myp` passes a `@struct`, so the dispatch in the grafted
/// call and in the surviving definition alike keeps the one arm that type
/// selects. The counter-factual is the definition's own dispatch, which a
/// rewrite of the call alone leaves whole, fallback arm and all.
#[test]
fn a_type_of_dispatch_over_a_proven_container_keeps_one_arm() {
    let src = format!("{MYP} (let [s @{{:x 0}}] (myp s :x 9))");
    let (hir, arena) = compiled(&src);
    assert_eq!(count_calls_to(&hir, &arena, "myp"), 0, "the call grafts");
    assert!(
        count_ops(&hir, &arena, "%put-struct-mut") > 0,
        "the @struct arm stays"
    );
    assert_eq!(
        count_calls_to(&hir, &arena, "dynamic-put"),
        0,
        "no fallback arm is left anywhere"
    );
}

/// `g` is called with a `@struct` and an `@array`, so its parameter is unknown
/// and the dispatch in its definition keeps every arm.
#[test]
fn an_unproven_container_keeps_the_dispatch() {
    let src = format!("{MYP} (defn g [c] (myp c :x 9)) (g @{{:x 0}}) (g @[1])");
    let (hir, arena) = compiled(&src);
    assert!(
        count_calls_to(&hir, &arena, "dynamic-put") > 0,
        "an unproven container leaves the fallback arm"
    );
}

/// The stdlib's container wrappers take the same path from another unit: each
/// call on a proven container ends as the one op that container's arm reaches,
/// with no fallback left to raise.
#[test]
fn a_stdlib_container_wrapper_on_a_proven_container_is_one_op() {
    for (src, op, wrapper) in [
        ("(put {:a 1} :b 2)", "%put-struct", "put"),
        ("(push [1 2] 3)", "%push-array", "push"),
        ("(add (set 1 2) 3)", "%add-set", "add"),
        ("(push \"ab\" \"c\")", "%string-push", "push"),
        (
            "(defn g [] (let [s @\"\"] (push s \"x\") s)) (g)",
            "%string-push-mut",
            "push",
        ),
    ] {
        let (hir, arena) = compiled(src);
        assert_eq!(
            count_calls_to(&hir, &arena, wrapper),
            0,
            "{src}: {wrapper} is gone"
        );
        assert!(
            count_ops(&hir, &arena, op) > 0,
            "{src}: {op} is what is left"
        );
        assert_eq!(
            count_emits(&hir),
            0,
            "{src}: no fallback arm is left to raise"
        );
    }
}

/// A guard certain to raise ends its sequence. The narrowing gives the `%add`
/// after it an operand no value reaches, and the contract would reject that
/// operand; the fold deletes the `%add` first, so the program still compiles
/// and raises at run time.
#[test]
fn a_certain_raise_cuts_what_follows_and_the_program_compiles() {
    for src in ["(+ \"a\" 1)", "(< \"a\" 1)", "(- :k)"] {
        let (hir, arena) = compiled(src);
        assert!(count_emits(&hir) > 0, "{src}: the raise stays");
        for op in ["%add", "%lt", "%sub"] {
            assert_eq!(
                count_ops(&hir, &arena, op),
                0,
                "{src}: no {op} survives the raise"
            );
        }
    }
}

/// `g` is only ever called with a string, so `(%int? b)` decides false and the
/// branch holding `%bit-and` is deleted before the contract is checked. The
/// counter-factual: the branch stays, its `b` is a Bottom, and the contract
/// rejects a site no value reaches.
#[test]
fn a_contradicting_guard_deletes_its_branch() {
    let src = "(defn g [b] (if (%int? b) (%bit-and b 1) 0)) (g \"str\")";
    let (hir, arena) = compiled(src);
    assert_eq!(
        count_ops(&hir, &arena, "%bit-and"),
        0,
        "{src}: the branch is gone"
    );
}

/// A deletion can narrow a join, and a later op may prove only against the
/// narrower type, so inference runs again over what the fold left. `y` joins an
/// int and a string until the fold deletes the string branch.
#[test]
fn a_deletion_that_narrows_a_join_proves_a_later_op() {
    let src = "(defn h [a] (let [y (if (%int? a) a \"s\")] (%bit-and y 1))) (h 3)";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// `number?` is grafted too, so the guard reads the `(match (type-of x) …)` it
/// became. `f` makes `h` a value, so `x` proves nothing but the guard.
#[test]
fn a_guard_proves_through_a_grafted_predicate() {
    let src = "(defn h [x] (when (%not (number? x)) (error :x)) (%add x 1)) (def f h) (f 1)";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// `(not (integer? x))` grafts to `%not` over a local bound to the grafted
/// `integer?`, and the guard reads through that local to `x`.
#[test]
fn a_guard_proves_through_the_let_a_graft_introduces() {
    let src = "(defn h [x] (when (not (integer? x)) (error :x)) (%bit-and x 1)) (def f h) (f 3)";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// The `each` macro binds a cell inside the `:array` arm, and functionalization
/// may hoist it out. The arm is dead for an int, and deleting it must not orphan
/// a binding the rest of the tree reads.
#[test]
fn a_dead_arm_that_binds_a_hoisted_cell_compiles() {
    let src = "(defn f [x] (match (type-of x) :array (each v x (println v)) _ :none)) (f 1)";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// The deleted guards were the only errors `h`'s body could raise, and the
/// re-propagation drops the bit. The analyzer's own signal for `h` is not
/// consulted here.
#[test]
fn a_dissolved_body_raises_nothing() {
    let src = "(defn h [a b] (+ a b)) (h 1 2)";
    let (hir, _) = compiled(src);
    let mut bodies = Vec::new();
    lambda_bodies(&hir, &mut bodies);
    assert!(!bodies.is_empty(), "{src}: h's definition survives");
    for sig in bodies {
        assert!(
            !sig.bits.intersects(crate::value::SIG_ERROR),
            "{src}: a dissolved body raises nothing"
        );
    }
}

fn lambda_bodies(h: &Hir, out: &mut Vec<crate::signals::Signal>) {
    if let HirKind::Lambda { body, .. } = &h.kind {
        out.push(body.signal);
    }
    h.for_each_child(|c| lambda_bodies(c, out));
}
