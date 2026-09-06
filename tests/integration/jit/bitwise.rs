// audited: 2026-09-06
// src/jit/AGENTS.md
// What JIT-compiled bitwise and logical operations return, and what an
// expression spanning several blocks returns.

use super::*;

// =============================================================================
// Complex Expression Tests
// =============================================================================

#[test]
fn test_jit_conditional_arithmetic() {
    // fn(x) -> if (x = 0) then 1 else (x * 2)
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 4;
    func.num_captures = 0;
    func.signal = Signal::silent();

    // Entry: load arg, compare x == 0
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::compare(Reg(2), CmpOp::Eq, Reg(0), Reg(1)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(2),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    // Then: return 1
    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(3),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());

    // Else: return x * 2
    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(2),
        },
        span(),
    ));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::binop(Reg(3), BinOp::Mul, Reg(0), Reg(1)),
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(3)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    // Test x = 0 -> 1
    let result = compile_and_call(&func, &[Value::int(0)]).unwrap();
    assert_eq!(result.as_int(), Some(1));

    // Test x = 5 -> 10
    let result2 = compile_and_call(&func, &[Value::int(5)]).unwrap();
    assert_eq!(result2.as_int(), Some(10));
}

#[test]
fn test_jit_chained_arithmetic() {
    // fn(a, b, c) -> (a + b) * c
    let mut func = LirFunction::new(Arity::Exact(3));
    func.num_regs = 5;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(load_arg(Reg(2), 2));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::binop(Reg(3), BinOp::Add, Reg(0), Reg(1)),
        span(),
    ));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::binop(Reg(4), BinOp::Mul, Reg(3), Reg(2)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(4)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // (2 + 5) * 6 = 42
    let result = compile_and_call(&func, &[Value::int(2), Value::int(5), Value::int(6)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

// =============================================================================
// Bitwise Operation Tests
// =============================================================================

#[test]
fn test_jit_bit_and() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::binop(Reg(2), BinOp::BitAnd, Reg(0), Reg(1)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // 0b1111 & 0b1010 = 0b1010 = 10
    let result = compile_and_call(&func, &[Value::int(15), Value::int(10)]).unwrap();
    assert_eq!(result.as_int(), Some(10));
}

#[test]
fn test_jit_bit_or() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::binop(Reg(2), BinOp::BitOr, Reg(0), Reg(1)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // 0b1100 | 0b0011 = 0b1111 = 15
    let result = compile_and_call(&func, &[Value::int(12), Value::int(3)]).unwrap();
    assert_eq!(result.as_int(), Some(15));
}

#[test]
fn test_jit_shl() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::binop(Reg(2), BinOp::Shl, Reg(0), Reg(1)),
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    // 1 << 4 = 16
    let result = compile_and_call(&func, &[Value::int(1), Value::int(4)]).unwrap();
    assert_eq!(result.as_int(), Some(16));
}

// =============================================================================
// Logical Operation Tests
// =============================================================================

#[test]
fn test_jit_not_true() {
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

    let result = compile_and_call(&func, &[Value::TRUE]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_not_false() {
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

    let result = compile_and_call(&func, &[Value::FALSE]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_not_nil() {
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

    let result = compile_and_call(&func, &[Value::NIL]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}
