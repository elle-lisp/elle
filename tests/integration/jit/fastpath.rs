// audited: 2026-09-06
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
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::unary(Reg(1), UnaryOp::Neg, Reg(0)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(-42)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_bit_not_zero() {
    // fn(x) -> ~x, bitwise NOT of 0 should be -1
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::unary(Reg(1), UnaryOp::BitNot, Reg(0)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result.as_int(), Some(-1));
}

#[test]
fn test_jit_not_integer_zero() {
    // fn(x) -> not(x), 0 is truthy in Elle so not(0) = false
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::unary(Reg(1), UnaryOp::Not, Reg(0)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result, Value::FALSE);
}

#[test]
fn test_jit_not_empty_list() {
    // fn(x) -> not(x), empty list is truthy in Elle so not(()) = false
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::unary(Reg(1), UnaryOp::Not, Reg(0)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::EMPTY_LIST]).unwrap();
    assert_eq!(result, Value::FALSE);
}

// =============================================================================
// Fiber + JIT Gate Tests
// =============================================================================

#[test]
fn test_jit_accepts_yields_errors_signal() {
    // Signal::yields_errors() has may_suspend() = true.
    // The JIT gate now accepts this via side-exit — yielding functions
    // can be JIT-compiled and will side-exit to the interpreter on yield.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::yields_errors();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func, None, Vec::new());
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
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::errors();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func, None, Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept errors-only signal: {:?}",
        result
    );
}
