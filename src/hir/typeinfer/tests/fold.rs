// audited: 2026-10-20
//! What the fold deletes after inference: a guard, a `cond` clause, an `and` or `or` operand, or a dispatch arm the types decide.
//!
//! docs/impl/graft.md
//!
//! What it keeps: the statements after a raise, which a resume reaches.

use super::{
    compile_booted, compiled, count_calls_to, count_emits, count_intrinsics, count_ops, run_booted,
};
use crate::hir::{Hir, HirKind};
use crate::value::Value;

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

/// How many statements before a sequence's tail are immediate literals, which
/// is what a decided guard leaves where it stood.
fn count_literal_statements(h: &Hir) -> usize {
    let mut n = 0;
    if let HirKind::Begin(items) = &h.kind {
        n += items
            .iter()
            .take(items.len().saturating_sub(1))
            .filter(|i| {
                matches!(
                    i.kind,
                    HirKind::Nil
                        | HirKind::EmptyList
                        | HirKind::Bool(_)
                        | HirKind::Int(_)
                        | HirKind::Float(_)
                        | HirKind::Keyword(_)
                )
            })
            .count();
    }
    h.for_each_child(|c| n += count_literal_statements(c));
    n
}

/// The `nil` a decided guard leaves is a statement with no effect and no
/// reader, so it goes, and a sequence left with its tail alone is the tail.
/// The counter-factual is that `nil` kept as a statement, which the lowerer
/// stored and popped on every run, so a wrapper whose guards both went still
/// cost two stores per call.
#[test]
fn a_decided_guard_leaves_no_statement_behind() {
    for src in [
        "(defn g [b] (when (%not (%int? b)) (error :x)) (%mul 2 b)) (g 5)",
        "(defn h [a b] (+ a b)) (h 1 2)",
    ] {
        let (hir, _) = compiled(src);
        assert_eq!(count_emits(&hir), 0, "{src}: every guard is gone");
        assert_eq!(
            count_literal_statements(&hir),
            0,
            "{src}: no guard left a literal statement behind"
        );
    }
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

/// A guard certain to raise keeps the statements after it, because a parent
/// may resume the raise. The op after it reads the operand the guard rejected,
/// which no type proves, and the contract check skips it there.
///
/// The counter-factual: the fold deleted the op, so a resumed fiber returned
/// the recovery value instead of running the rest of the body.
#[test]
fn a_certain_raise_keeps_what_follows_and_the_program_compiles() {
    for (src, op) in [("(+ \"a\" 1)", "%add"), ("(- :k)", "%sub")] {
        let (hir, arena) = compiled(src);
        assert!(count_emits(&hir) > 0, "{src}: the raise stays");
        assert!(
            count_ops(&hir, &arena, op) > 0,
            "{src}: {op} stays for a resume"
        );
    }
    compiled("(< \"a\" 1)");
}

/// An `:error` is resumable, so the statement after a raise runs when the
/// parent resumes the child. The counter-factual: the fold cut `:done`, and
/// the resume answered with the value it passed in.
#[test]
fn a_resumed_raise_runs_the_statement_after_it() {
    let src = "(def f (fiber/new (fn [] (emit :error :stop) :done) |:error|))
               (fiber/resume f)
               (fiber/resume f :again)";
    assert_eq!(run_booted(src), Value::keyword("done"), "{src}");
}

/// The inner block's own `break` leaves it for the statement after it, so the
/// guard's branch does not diverge and proves nothing after the guard. `f` is
/// used as a value, so `x` is unknown.
///
/// The counter-factual: a block whose tail is a `break` read as diverging, so
/// the guard proved `x` an int for the `%bit-and`, which then compiled over a
/// string.
#[test]
fn a_block_its_own_break_leaves_proves_nothing_after_it() {
    let src = "(defn f [x]
                 (when (%not (%int? x)) (block :b (break :b nil)))
                 (%bit-and x 1))
               (def g f)
               (g \"s\")";
    let err = compile_booted(src).expect_err("x is not proven an int");
    assert!(err.contains("%bit-and"), "{src}: {err}");
}

/// A `cond` clause whose test decides false goes, and so does its body. The
/// counter-factual: the clause stayed, its `x` met a string with an int at
/// Bottom, and the contract rejected a site no value reaches.
#[test]
fn a_cond_clause_that_decides_false_goes() {
    let src = "(defn f [x] (cond (%int? x) (%bit-and x 1) true 0)) (f \"s\")";
    let (hir, arena) = compiled(src);
    assert_eq!(count_ops(&hir, &arena, "%bit-and"), 0, "{src}");
}

/// A clause whose test decides true is the last one that can run, so every
/// clause after it goes. The counter-factual: the second clause stayed, and
/// its `%bit-and` over a string was rejected.
#[test]
fn a_cond_clause_that_decides_true_ends_the_cond() {
    let src = "(defn f [x y] (cond (%int? x) 1 (%bit-and y 1) 2 true 3)) (f 5 \"s\")";
    let (hir, arena) = compiled(src);
    assert_eq!(count_ops(&hir, &arena, "%bit-and"), 0, "{src}");
}

/// An `and` stops at an operand that decides it false, and an `or` at one that
/// decides it true: nothing after it runs. The counter-factual: the operand
/// after it stayed, and its `%bit-and` over a string was rejected.
#[test]
fn an_and_or_an_or_stops_at_the_operand_that_decides_it() {
    for src in [
        "(defn f [x] (and (%int? x) (%bit-and x 1))) (f \"s\")",
        "(defn f [x] (or (%not (%int? x)) (%bit-and x 1))) (f \"s\")",
        "(inf? nil)",
        "(nan? :k)",
    ] {
        compiled(src);
    }
}

/// An operand before the last that decides an `and` true is never its result,
/// and running it has no effect, so it goes. The counter-factual: the proven
/// test stayed in the tree.
#[test]
fn a_true_operand_before_the_last_leaves_an_and() {
    let src = "(defn f [x] (and (%int? x) (%bit-and x 1))) (f 5)";
    let (hir, _) = compiled(src);
    assert_eq!(count_intrinsics(&hir, "%int?"), 0, "{src}");
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
    if let HirKind::Lambda(lambda) = &h.kind {
        out.push(lambda.body.signal);
    }
    h.for_each_child(|c| lambda_bodies(c, out));
}
