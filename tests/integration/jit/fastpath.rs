// audited: 2026-10-06
// src/jit/AGENTS.md
// The unary fast paths over each operand shape, and which signals the JIT
// accepts a function under.

use super::*;

// =============================================================================
// Unary Fast Path Tests
// =============================================================================

#[test]
fn test_jit_neg_negative() {
    // fn(x) -> -x with negative input
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::Neg, Reg(0)));
    let result = compile_and_call(&func, &[Value::int(-42)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_bit_not_zero() {
    // fn(x) -> ~x, bitwise NOT of 0 should be -1
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::BitNot, Reg(0)));
    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result.as_int(), Some(-1));
}

#[test]
fn test_jit_not_integer_zero() {
    // fn(x) -> not(x), 0 is truthy in Elle so not(0) = false
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::Not, Reg(0)));
    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result, Value::FALSE);
}

#[test]
fn test_jit_not_empty_list() {
    // fn(x) -> not(x), empty list is truthy in Elle so not(()) = false
    let func = unary(InstrRef::unary(Reg(1), UnaryOp::Not, Reg(0)));
    let result = compile_and_call(&func, &[Value::EMPTY_LIST]).unwrap();
    assert_eq!(result, Value::FALSE);
}

// =============================================================================
// Fiber + JIT Gate Tests
// =============================================================================

/// fn() -> 42, carrying `signal`.
fn forty_two(signal: Signal) -> LirOwned {
    function(
        Arity::Exact(0),
        1,
        signal,
        &[(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Int(42),
            }],
            Terminator::Return(Reg(0)),
        )],
    )
}

#[test]
fn test_jit_accepts_yields_errors_signal() {
    // Signal::yields_errors() has may_suspend() = true.
    // The JIT gate now accepts this via side-exit — yielding functions
    // can be JIT-compiled and will side-exit to the interpreter on yield.
    let func = forty_two(Signal::yields_errors());

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func.view());
    assert!(
        result.is_ok(),
        "JIT should accept yields_errors signal via side-exit: {:?}",
        result
    );
}

#[test]
fn test_jit_accepts_errors_only_signal() {
    // Signal::errors() has may_suspend() = false.
    // The JIT gate should accept this — fiber/new, fiber/status, etc.
    // have this signal and are safe to call from JIT code.
    let func = forty_two(Signal::errors());

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func.view());
    assert!(
        result.is_ok(),
        "JIT should accept errors-only signal: {:?}",
        result
    );
}
