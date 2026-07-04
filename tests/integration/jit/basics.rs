use super::*;

// =============================================================================
// Basic Tests
// =============================================================================

#[test]
fn test_jit_identity() {
    // fn(x) -> x
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_constant() {
    // fn() -> 42
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

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

    let result = compile_and_call(&func, &[]).unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_nil() {
    // fn() -> nil
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Nil,
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert!(result.is_nil());
}

#[test]
fn test_jit_bool_true() {
    // fn() -> true
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Bool(true),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert_eq!(result.as_bool(), Some(true));
}

#[test]
fn test_jit_bool_false() {
    // fn() -> false
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Bool(false),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert_eq!(result.as_bool(), Some(false));
}

#[test]
fn test_jit_empty_list() {
    // fn() -> ()
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::EmptyList,
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[]).unwrap();
    assert!(result.is_empty_list());
}
