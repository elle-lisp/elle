// audited: 2026-10-21
// Signal narrowing after type inference: what a call to a primitive drops, and what a same-named function keeps.
//
// docs/impl/typeinfer.md
//
// Narrowing rewrites the signal of a call node, and leaves every function's
// inferred signal alone. So these tests compile a whole file and read the
// signal of its entry function, where a top-level call's signal lands.

use super::*;

/// The signal of the entry function a file compiles to.
fn entry_signal(source: &str) -> Signal {
    let mut rt = elle::runtime::Runtime::new();
    let (_vm, symbols, cctx) = rt.parts();
    let unit = elle::compile_file(source, symbols, cctx, "<narrowing>").expect("compiles");
    unit.entry().signal()
}

// The control: the primitive `length` over a string cannot raise, so the call
// carries no `:error`. Without it, the two tests below would pass for a
// narrowing that never ran.
#[test]
fn a_primitive_call_whose_operand_types_rule_out_the_error_drops_it() {
    let signal = entry_signal("(length \"abc\")");
    assert!(
        !signal.may_error(),
        "length over a string is narrowed: {signal:?}"
    );
}

// Each function below calls itself, so the graft leaves the top-level call a
// call. A row that matched the callee's name would read the string operand,
// drop `:error`, and claim the entry cannot raise while the call raises.
#[test]
fn a_function_named_length_keeps_the_error_its_body_raises() {
    let signal = entry_signal(
        "(defn length [x]
           (if (string? x) (length 0) (error {:error :mine :message \"mine\"})))
         (length \"abc\")",
    );
    assert!(
        signal.may_error(),
        "a user length that raises is not the primitive: {signal:?}"
    );
}

#[test]
fn a_function_named_push_keeps_the_error_its_body_raises() {
    let signal = entry_signal(
        "(defn push [a x]
           (if (array? a) (push 0 x) (error {:error :mine :message \"mine\"})))
         (push @[1] 2)",
    );
    assert!(
        signal.may_error(),
        "a user push that raises is not the primitive: {signal:?}"
    );
}
