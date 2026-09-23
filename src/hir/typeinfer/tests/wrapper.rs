// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/intrinsics.md
//! The arithmetic wrapper lowering: a call to `+`, `-`, `*`, `<`, `>`, `<=`,
//! `>=` or `=` over two proven numbers becomes its `%`-op, and every other
//! shape keeps the wrapper call.
//!
//! These compile against a booted runtime, because the wrappers are stdlib
//! closures: without the stdlib a source's `+` names nothing.

use super::count_calls_to;
use crate::hir::{BindingArena, Hir, HirKind};

/// The eight wrappers and the op each lowers to.
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

/// Compile `src` through the file front end of a runtime with the stdlib
/// loaded, returning the regularized HIR.
fn compile(src: &str) -> Result<(Hir, BindingArena), String> {
    let mut rt = crate::runtime::Runtime::new();
    let (_vm, symbols, cctx) = rt.parts();
    crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<test>")
}

/// How many `%`-intrinsic nodes named `op` the tree holds.
fn count_intrinsics(h: &Hir, op: &str) -> usize {
    let mut n =
        usize::from(matches!(&h.kind, HirKind::Intrinsic { op: this, .. } if this.name() == op));
    h.for_each_child(|c| n += count_intrinsics(c, op));
    n
}

/// Compile `src`, then report how many calls to `wrapper` and how many `op`
/// nodes survive.
fn survivors(src: &str, wrapper: &str, op: &str) -> (usize, usize) {
    let (hir, arena) = compile(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
    (
        count_calls_to(&hir, &arena, wrapper),
        count_intrinsics(&hir, op),
    )
}

#[test]
fn each_wrapper_over_two_proven_ints_lowers_to_its_op() {
    for (wrapper, op) in WRAPPERS {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h 1 2)");
        assert_eq!(
            survivors(&src, wrapper, op),
            (0, 1),
            "{src}: both operands are proven ints, so the call becomes one {op}"
        );
    }
}

#[test]
fn an_int_and_a_float_lower_as_two_numbers() {
    for (wrapper, op) in WRAPPERS {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h 1 2.5)");
        assert_eq!(
            survivors(&src, wrapper, op),
            (0, 1),
            "{src}: an int and a float are both numbers, so the call becomes {op}"
        );
    }
}

/// The issue's own program. Every operand proves: `n` is an int from the
/// caller, `(- n 1)` over an int is an int, and each self-call returns the
/// body type, which is an int.
#[test]
fn the_wrapper_spelled_fib_holds_no_wrapper_call() {
    let src = "(defn fib [n] (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 30)";
    let (hir, arena) = compile(src).expect("fib compiles");
    for wrapper in ["+", "-", "<"] {
        assert_eq!(
            count_calls_to(&hir, &arena, wrapper),
            0,
            "fib must hold no call to {wrapper}"
        );
    }
    assert_eq!(count_intrinsics(&hir, "%lt"), 1);
    assert_eq!(count_intrinsics(&hir, "%sub"), 2);
    assert_eq!(count_intrinsics(&hir, "%add"), 1);
}

/// The counter-factual: one caller passes a keyword, so the parameter joins to
/// unknown and nothing proves the operand.
#[test]
fn an_unproven_operand_keeps_the_wrapper_call() {
    for (wrapper, op) in WRAPPERS {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h 1 2) (h :k 2)");
        assert_eq!(
            survivors(&src, wrapper, op),
            (1, 0),
            "{src}: a is unproven, so the wrapper call stays"
        );
    }
}

/// `%lt` orders two strings and `%eq` compares them, but the lowering admits
/// only numbers (docs/intrinsics.md), so a string pair keeps the call.
#[test]
fn two_strings_keep_the_comparison_call() {
    for (wrapper, op) in WRAPPERS.iter().skip(3) {
        let src = format!("(defn h [a b] ({wrapper} a b)) (h \"a\" \"b\")");
        assert_eq!(
            survivors(&src, wrapper, op),
            (1, 0),
            "{src}: two strings are not two numbers, so the wrapper call stays"
        );
    }
}

#[test]
fn three_operands_keep_the_call() {
    for (wrapper, op) in WRAPPERS {
        let src = format!("(defn h [a b c] ({wrapper} a b c)) (h 1 2 3)");
        assert_eq!(
            survivors(&src, wrapper, op),
            (1, 0),
            "{src}: the op takes two operands, so a three-operand call stays"
        );
    }
}

#[test]
fn a_spliced_operand_keeps_the_call() {
    let src = "(defn h [a] (+ a ;[1])) (h 1)";
    assert_eq!(
        survivors(src, "+", "%add"),
        (1, 0),
        "a spliced operand has no type of its own, so the call stays"
    );
}

/// The counter-factual for the callee: a name is not the wrapper. A file-level
/// `+` and a parameter named `+` are user bindings, and lowering a call to
/// either would replace the user's function with addition.
#[test]
fn a_user_binding_named_plus_keeps_its_call() {
    let file_level = "(defn + [a b] (* a b)) (+ 2 3)";
    assert_eq!(
        survivors(file_level, "+", "%add"),
        (1, 0),
        "{file_level}: + is the user's function"
    );
    let parameter = "(defn h [+ a b] (+ a b)) (h (fn [x y] (string x y)) 1 2)";
    assert_eq!(
        survivors(parameter, "+", "%add"),
        (1, 0),
        "{parameter}: + is h's parameter"
    );
}

/// The result of `-` over two proven ints is an int, so it discharges an op
/// that needs one. While the wrapper's result typed as Number, `%bit-and`
/// rejected this.
#[test]
fn a_wrapper_result_over_ints_proves_an_int_op() {
    let ints = "(defn f [n] (%bit-and (- n 1) 1)) (f 5)";
    assert!(
        compile(ints).is_ok(),
        "{ints}: (- n 1) over an int is an int"
    );
    let floats = "(defn f [n] (%bit-and (- n 1) 1)) (f 5.5)";
    let err = compile(floats).expect_err("a float n makes (- n 1) a Number");
    assert!(err.contains("not a proven int"), "{floats}: {err}");
}

#[test]
fn the_empty_sum_and_product_are_ints() {
    for src in ["(%bit-and (+) 1)", "(%bit-and (*) 1)"] {
        assert!(
            compile(src).is_ok(),
            "{src}: an empty sum or product is an int"
        );
    }
}
