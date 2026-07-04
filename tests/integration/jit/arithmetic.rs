use super::*;

// =============================================================================
// Arithmetic Tests
// =============================================================================

#[test]
fn test_jit_add() {
    // fn(x, y) -> x + y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Add,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(10), Value::int(32)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_sub() {
    // fn(x, y) -> x - y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Sub,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(50), Value::int(8)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_mul() {
    // fn(x, y) -> x * y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Mul,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(6), Value::int(7)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_div() {
    // fn(x, y) -> x / y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Div,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(84), Value::int(2)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_rem() {
    // fn(x, y) -> x % y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::BinOp {
            dst: Reg(2),
            op: BinOp::Rem,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(47), Value::int(5)]).unwrap();
    assert_eq!(result.as_int(), Some(2));
}

#[test]
fn test_jit_neg() {
    // fn(x) -> -x
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::UnaryOp {
            dst: Reg(1),
            op: UnaryOp::Neg,
            src: Reg(0),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(-42));
}

// =============================================================================
// Comparison Tests
// =============================================================================

#[test]
fn test_jit_lt_true() {
    // fn(x, y) -> x < y
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Lt,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(1), Value::int(2)]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_lt_false() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Lt,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(2), Value::int(1)]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_eq() {
    let mut func = LirFunction::new(Arity::Exact(2));
    func.num_regs = 3;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(load_arg(Reg(1), 1));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Compare {
            dst: Reg(2),
            op: CmpOp::Eq,
            lhs: Reg(0),
            rhs: Reg(1),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42), Value::int(42)]).unwrap();
    assert_eq!(result.as_bool(), Some(true));

    let result2 = compile_and_call(&func, &[Value::int(42), Value::int(43)]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}
