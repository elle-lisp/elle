use super::*;

// =============================================================================
// Control Flow Tests
// =============================================================================

#[test]
fn test_jit_branch_true() {
    // fn(x) -> if x then 1 else 0
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    // Entry block: load arg, branch on x
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    // Then block: return 1
    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    // Else block: return 0
    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    // Test with true
    let result = compile_and_call(&func, &[Value::TRUE]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

#[test]
fn test_jit_branch_false() {
    // fn(x) -> if x then 1 else 0
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    // Test with false
    let result = compile_and_call(&func, &[Value::FALSE]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_nil() {
    // nil is falsy
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::NIL]).unwrap();
    assert_eq!(result.as_int(), Some(0));
}

#[test]
fn test_jit_branch_integer_truthy() {
    // Non-zero integers are truthy
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        span(),
    );

    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(0),
        },
        span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());

    func.blocks.push(entry);
    func.blocks.push(then_block);
    func.blocks.push(else_block);
    func.entry = Label(0);

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result.as_int(), Some(1));
}

// =============================================================================
// Error Handling Tests
// =============================================================================

#[test]
fn test_jit_accepts_yielding() {
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::yields();

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
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept yielding functions via side-exit: {:?}",
        result
    );
}

#[test]
fn test_jit_call_compiles() {
    // Test that Call instruction compiles
    let mut func = LirFunction::new(Arity::Exact(1));
    func.num_regs = 2;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(load_arg(Reg(0), 0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Call {
            dst: Reg(1),
            func: Reg(0),
            args: vec![],
            arity_checked: false,
            region: elle::hir::region::StaticRegion::new(2).unwrap(),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    // Call should now compile successfully
    assert!(result.is_ok(), "Call should compile: {:?}", result);
}

#[test]
fn test_jit_rejects_make_closure() {
    // MakeClosure is rejected at the gate — the per-compilation cost of
    // emitting module closures' bytecodes is too high. Functions with
    // MakeClosure fall back to the interpreter.
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 1;
    func.num_captures = 0;
    func.signal = Signal::silent();

    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::MakeClosure {
            dst: Reg(0),
            closure_id: elle::lir::ClosureId(0),
            captures: vec![],
            // A real per-execution slot (>= 2). The lowerer assigns real slots
            // to allocating instructions.
            region: elle::hir::region::StaticRegion::new(2).unwrap(),
        },
        span(),
    ));
    entry.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), span());
    func.blocks.push(entry);
    func.entry = Label(0);

    let compiler = JitCompiler::new().unwrap();
    let result = compiler.compile(&func, None, std::collections::HashMap::new(), Vec::new());
    assert!(
        matches!(result, Err(elle::jit::JitError::UnsupportedInstruction(_))),
        "MakeClosure should be rejected: {:?}",
        result,
    );
}
