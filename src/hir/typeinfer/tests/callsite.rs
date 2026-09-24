// audited: 2026-09-23
// src/hir/AGENTS.md
// docs/impl/typeinfer.md
//! What a call contributes to its callee's parameters when the operands do not
//! line up with them one to one, and what a call to a name the program rebound
//! proves.
//!
//! Each case compiled before its fix and proved a type no value has. The fold
//! deletes code on these types, so each wrong answer would pick a branch the
//! program does not take.

use super::compile_booted;

fn rejected(src: &str) -> String {
    compile_booted(src)
        .err()
        .unwrap_or_else(|| panic!("{src} must be rejected"))
}

fn compiles(src: &str) {
    compile_booted(src).unwrap_or_else(|e| panic!("{src} must compile: {e}"));
}

/// The counter-factual: the join read the operand in the rest parameter's
/// position, so `more` took the type of the `2` inside the list.
#[test]
fn a_rest_parameter_is_the_list_of_the_extra_operands() {
    let err = rejected("(defn f [a & more] (%bit-and more 1)) (f 1 2)");
    assert!(err.contains("%bit-and"), "{err}");
}

/// A call that gives an `&opt` parameter no operand binds it to nil. The
/// counter-factual: the join skipped the missing operand, so the one call that
/// passed an int proved `b` for both.
#[test]
fn a_missing_optional_operand_contributes_nil() {
    compiles("(defn f [a &opt b] (%bit-and b 1)) (f 1 2)");
    let err = rejected("(defn f [a &opt b] (%bit-and b 1)) (f 1 2) (f 1)");
    assert!(err.contains("%bit-and"), "{err}");
}

/// The splice spreads two values, so the `2` written after it lands in no
/// parameter at all. The counter-factual: the join read operands by their
/// written position, so `b` took the `2` and the string reached `%bit-and`.
#[test]
fn an_operand_at_or_after_a_splice_proves_nothing() {
    compiles("(defn f [a b] (%bit-and a 1)) (f 1 ;[2])");
    let err = rejected("(defn f [a b] (%bit-and b 1)) (f ;[1 \"s\"] 2)");
    assert!(err.contains("%bit-and"), "{err}");
}

/// A parameter named `length` holds the function the caller passed. The
/// counter-factual: the result rule matched the primitive table by name, so
/// the call was typed an int.
#[test]
fn a_parameter_named_like_a_primitive_is_an_unknown_function() {
    let err = rejected(
        "(defn h [length s] (%bit-and (length s) 1)) \
         (h (fn [s] \"x\") \"abc\")",
    );
    assert!(err.contains("%bit-and"), "{err}");
}

/// A parameter named `number?` holds the predicate the caller passed, which
/// here answers true for a string. The counter-factual: guard recognition
/// matched the predicate by name, so the guard proved `x` a number.
#[test]
fn a_parameter_named_like_a_predicate_proves_nothing() {
    let err = rejected(
        "(defn h [number? x] (when (%not (number? x)) (error :x)) (%add x 1)) \
         (def f h) (f (fn [x] true) \"s\")",
    );
    assert!(err.contains("%add"), "{err}");
}
