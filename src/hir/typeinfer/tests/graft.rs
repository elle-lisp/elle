// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/impl/graft.md
//! Which calls become their callee's body: a known, non-recursive function at
//! an arity its body closes for, and nothing else.
//!
//! These compile against a booted runtime, because most of the callees are
//! stdlib closures: without the stdlib a source's `+` names nothing.

use super::{compile_booted, count_calls_to, count_emits, count_intrinsics, count_ops};
use crate::hir::{BindingArena, Hir, HirKind};

/// The eight wrappers and the op each one's two-operand arm reaches.
const WRAPPERS: [(&str, &str); 8] = [
    ("+", "%add"),
    ("-", "%sub"),
    ("*", "%mul"),
    ("<", "%lt"),
    (">", "%gt"),
    ("<=", "%le"),
    (">=", "%ge"),
    ("=", "%eq"),
];

fn compiled(src: &str) -> (Hir, BindingArena) {
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"))
}

/// Over two proven ints the wrapper's guards and its number test are all
/// decided, so the call dissolves to the op and nothing that could raise is
/// left. The counter-factual is the wrapper call itself, which a named call
/// keeps however well its operands are proven.
#[test]
fn each_wrapper_over_two_proven_ints_leaves_its_op_and_no_guard() {
    for (wrapper, op) in WRAPPERS {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h 1 2)");
        let (hir, arena) = compiled(&src);
        assert_eq!(
            count_calls_to(&hir, &arena, wrapper),
            0,
            "{src}: {wrapper} grafts"
        );
        assert!(
            count_ops(&hir, &arena, op) > 0,
            "{src}: {op} is what is left"
        );
        assert_eq!(count_emits(&hir), 0, "{src}: no guard is left to raise");
    }
}

#[test]
fn an_int_and_a_float_dissolve_as_two_numbers() {
    for (wrapper, op) in WRAPPERS {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h 1 2.5)");
        let (hir, arena) = compiled(&src);
        assert_eq!(
            count_calls_to(&hir, &arena, wrapper),
            0,
            "{src}: {wrapper} grafts"
        );
        assert!(
            count_ops(&hir, &arena, op) > 0,
            "{src}: {op} is what is left"
        );
        assert_eq!(count_emits(&hir), 0, "{src}: no guard is left to raise");
    }
}

/// One caller passes a keyword, so the definition's parameter joins to unknown.
/// The call still grafts, and what it keeps is the runtime check: the guards
/// for arithmetic, and the comparison through the op as a value for the rest.
#[test]
fn an_unproven_operand_grafts_and_keeps_its_runtime_check() {
    for (wrapper, dynamic) in [("+", None), ("-", None), ("*", None)].into_iter().chain(
        [
            ("<", "dynamic-lt"),
            (">", "dynamic-gt"),
            ("<=", "dynamic-le"),
            (">=", "dynamic-ge"),
            ("=", "dynamic-eq"),
        ]
        .map(|(w, d)| (w, Some(d))),
    ) {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h 1 2) (h :k 2)");
        let (hir, arena) = compiled(&src);
        assert_eq!(
            count_calls_to(&hir, &arena, wrapper),
            0,
            "{src}: {wrapper} grafts"
        );
        match dynamic {
            None => assert!(count_emits(&hir) > 0, "{src}: the guards stay"),
            Some(d) => assert!(
                count_calls_to(&hir, &arena, d) > 0,
                "{src}: the comparison through {d} stays"
            ),
        }
    }
}

/// The general arm of `+`, `-` and `*` holds a `letrec` walker, which does not
/// close, so three operands keep the call.
#[test]
fn three_operands_to_an_arithmetic_wrapper_keep_the_call() {
    for wrapper in ["+", "-", "*"] {
        let src = format!("(defn h [a b c] ({wrapper} a b c)) (h 1 2 3)");
        let (hir, arena) = compiled(&src);
        assert!(
            count_calls_to(&hir, &arena, wrapper) > 0,
            "{src}: the general arm does not graft"
        );
    }
}

/// A comparison's general arm closes, so three operands graft. `apply` is a
/// macro, so the rest of the chain is compared through the spliced call
/// `(< b ;more)`, which is the one call to the wrapper left.
///
/// The counter-factual: counting a call to `apply` passed on no tree at all,
/// because no such call exists once the macro expands.
#[test]
fn three_operands_to_a_comparison_graft_to_one_spliced_call() {
    for wrapper in ["<", ">", "<=", ">=", "="] {
        let src = format!("(defn h [a b c] ({wrapper} a b c)) (h 1 2 3)");
        let (hir, arena) = compiled(&src);
        let (spliced, whole) = calls_by_splice(&hir, &arena, wrapper);
        assert_eq!(whole, 0, "{src}: the call {wrapper} a b c grafts");
        assert!(
            spliced > 0,
            "{src}: the tail is compared through a spliced call to {wrapper}"
        );
    }
}

/// How many calls to `name` the tree holds with a spliced operand, and how many
/// without one.
fn calls_by_splice(h: &Hir, arena: &BindingArena, name: &str) -> (usize, usize) {
    let mut counts = (0, 0);
    if let HirKind::Call { func, args, .. } = &h.kind {
        if let Some(b) = crate::hir::typeinfer::unwrap_callee_binding(func) {
            if arena.get(b).name == crate::value::SymbolId::of(name) {
                if args.iter().any(|a| a.spliced) {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                }
            }
        }
    }
    h.for_each_child(|c| {
        let (s, w) = calls_by_splice(c, arena, name);
        counts.0 += s;
        counts.1 += w;
    });
    counts
}

#[test]
fn a_spliced_operand_keeps_the_call() {
    let src = "(defn h [a] (+ a ;[1])) (h 1)";
    let (hir, arena) = compiled(src);
    assert!(
        count_calls_to(&hir, &arena, "+") > 0,
        "{src}: a spliced operand leaves the arity unknown"
    );
}

/// A name is not a body. A file-level `+` is the user's function, and its call
/// grafts the user's body — multiplication — never the stdlib's addition. A
/// parameter named `+` holds whatever the caller passed.
#[test]
fn a_user_binding_named_plus_never_reaches_the_stdlib_body() {
    let file_level = "(defn + [a b] (* a b)) (+ 2 3)";
    let (hir, arena) = compiled(file_level);
    assert_eq!(
        count_ops(&hir, &arena, "%add"),
        0,
        "{file_level}: no addition"
    );
    assert!(
        count_ops(&hir, &arena, "%mul") > 0,
        "{file_level}: the user's body is what grafts"
    );
    let parameter = "(defn h [+ a b] (+ a b)) (h (fn [x y] (string x y)) 1 2)";
    let (hir, arena) = compiled(parameter);
    assert_eq!(
        count_ops(&hir, &arena, "%add"),
        0,
        "{parameter}: no addition"
    );
}

/// A function that calls itself never grafts, so its calls stay calls: the one
/// the program makes and the one its body makes.
#[test]
fn a_recursive_function_stays_a_call() {
    let src = "(defn f [n] (if (%lt n 1) 0 (f (%sub n 1)))) (f 3)";
    let (hir, arena) = compiled(src);
    assert_eq!(count_calls_to(&hir, &arena, "f"), 2, "{src}");
}

/// A function this unit defines grafts like a stdlib one, and its definition
/// keeps the proof its grafted callers gave it. The counter-factual: with the
/// call grafted away and nothing recorded, `x` has no call site, and `%mul x x`
/// in the surviving definition is rejected.
#[test]
fn a_same_unit_function_grafts_and_its_definition_still_proves() {
    let src = "(defn f [x] (%mul x x)) (f 3)";
    let (hir, arena) = compiled(src);
    assert_eq!(
        count_calls_to(&hir, &arena, "f"),
        0,
        "{src}: the call grafts"
    );
    let rejected = "(defn f [x] (%mul x x)) (f 3) (f \"s\")";
    let err = compile_booted(rejected).expect_err("a string caller still refutes x");
    assert!(err.contains("%mul"), "{rejected}: {err}");
}

/// A `(numeric!)` parameter is bound by a `let`, so its floor travels into the
/// caller. `g` is used as a value, so `y` proves nothing, and only the floor
/// proves the grafted `%mul`. The counter-factual: replacing `x` with `y`
/// drops the floor, and the grafted `%mul` is rejected.
#[test]
fn a_numeric_parameter_keeps_its_floor_in_the_graft() {
    let src = "(defn sq [x] (numeric!) (%mul x x)) (defn g [y] (sq y)) (def f g) (f 2)";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// The floor holds whatever the operand is, as it does for a caller of the
/// function: the declaration, not the flow, types the parameter.
///
/// The counter-factual: the `let` the graft binds `x` with met the string's
/// type with the floor at Bottom, which proves nothing, so the grafted `%mul`
/// was rejected where the call it replaced compiled.
#[test]
fn a_numeric_parameter_keeps_its_floor_against_an_operand_it_contradicts() {
    let src = "(defn sq [x] (numeric!) (%mul x x)) (sq \"a\")";
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// The issue's own program. `fib` calls itself, so it stays a call, and every
/// wrapper in its body grafts over an int `n` and dissolves.
#[test]
fn the_wrapper_spelled_fib_holds_no_wrapper_call_and_no_guard() {
    let src = "(defn fib [n] (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 30)";
    let (hir, arena) = compiled(src);
    for wrapper in ["+", "-", "<"] {
        assert_eq!(
            count_calls_to(&hir, &arena, wrapper),
            0,
            "fib holds no {wrapper}"
        );
    }
    assert_eq!(count_intrinsics(&hir, "%lt"), 1);
    assert_eq!(count_intrinsics(&hir, "%sub"), 2);
    assert_eq!(count_intrinsics(&hir, "%add"), 1);
    assert_eq!(count_emits(&hir), 0, "fib keeps no guard");
}

/// The result of a grafted `-` over two proven ints is the int its `%sub`
/// returns, so it discharges an op that needs one; over a float it does not.
#[test]
fn a_grafted_result_over_ints_proves_an_int_op() {
    let ints = "(defn f [n] (%bit-and (- n 1) 1)) (f 5)";
    compile_booted(ints).unwrap_or_else(|e| panic!("{ints} must compile: {e}"));
    let floats = "(defn f [n] (%bit-and (- n 1) 1)) (f 5.5)";
    let err = compile_booted(floats).expect_err("a float n makes (- n 1) a float");
    assert!(err.contains("not a proven int"), "{floats}: {err}");
}

/// `(+)` and `(*)` take the empty arm of the rest-list dispatch, a literal.
#[test]
fn the_empty_sum_and_product_are_ints() {
    for src in ["(%bit-and (+) 1)", "(%bit-and (*) 1)"] {
        compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
    }
}
