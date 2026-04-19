//! Tests for LIR to bytecode emission

use super::*;
use crate::syntax::Span;
use crate::value::Arity;

fn synthetic_span() -> Span {
    Span::synthetic()
}

#[test]
fn test_emit_simple() {
    let mut emitter = Emitter::new();

    let mut func = LirFunction::new(Arity::Exact(0));
    let mut block = BasicBlock::new(Label(0));
    block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        synthetic_span(),
    ));
    block.terminator = SpannedTerminator::new(Terminator::Return(Reg(0)), synthetic_span());
    func.blocks.push(block);
    func.entry = Label(0);

    let (bytecode, _, _) = emitter.emit(&func);
    assert!(!bytecode.instructions.is_empty());
}

#[test]
fn test_emit_branch() {
    let mut emitter = Emitter::new();

    let mut func = LirFunction::new(Arity::Exact(0));

    // Entry block
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Bool(true),
        },
        synthetic_span(),
    ));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(0),
            then_label: Label(1),
            else_label: Label(2),
        },
        synthetic_span(),
    );
    func.blocks.push(entry);

    // Then block
    let mut then_block = BasicBlock::new(Label(1));
    then_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::Int(1),
        },
        synthetic_span(),
    ));
    then_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), synthetic_span());
    func.blocks.push(then_block);

    // Else block
    let mut else_block = BasicBlock::new(Label(2));
    else_block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(2),
            value: LirConst::Int(2),
        },
        synthetic_span(),
    ));
    else_block.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), synthetic_span());
    func.blocks.push(else_block);

    func.entry = Label(0);

    let (bytecode, _, _) = emitter.emit(&func);
    assert!(!bytecode.instructions.is_empty());
    // Should have Jump instructions for control flow
    assert!(bytecode
        .instructions
        .iter()
        .any(|&b| b == Instruction::Jump as u8 || b == Instruction::JumpIfFalse as u8));
}

#[test]
fn test_yield_point_info_collected() {
    let mut emitter = Emitter::new();

    // fn() { yield 42; resume_value }
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 2;
    func.signal = crate::signals::Signal::yields();

    let mut b0 = BasicBlock::new(Label(0));
    b0.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(42),
        },
        synthetic_span(),
    ));
    b0.terminator = SpannedTerminator::new(
        Terminator::Emit {
            signal: crate::value::fiber::SIG_YIELD,
            value: Reg(0),
            resume_label: Label(1),
        },
        synthetic_span(),
    );

    let mut b1 = BasicBlock::new(Label(1));
    b1.instructions.push(SpannedInstr::new(
        LirInstr::LoadResumeValue { dst: Reg(1) },
        synthetic_span(),
    ));
    b1.terminator = SpannedTerminator::new(Terminator::Return(Reg(1)), synthetic_span());

    func.blocks = vec![b0, b1];
    func.entry = Label(0);

    let (bytecode, yield_points, _call_sites) = emitter.emit(&func);
    assert!(!bytecode.instructions.is_empty());
    assert_eq!(yield_points.len(), 1);
    assert!(yield_points[0].resume_ip > 0);
    // stack_regs should be empty — only Reg(0) was on stack, but it was
    // popped by the Yield. The remaining stack is empty.
    assert!(yield_points[0].stack_regs.is_empty());
}

#[cfg(feature = "jit")]
#[test]
fn test_yield_sentinel_distinct() {
    use crate::jit::dispatch::{TAIL_CALL_SENTINEL, YIELD_SENTINEL};
    use crate::jit::JitValue;
    assert_ne!(YIELD_SENTINEL, TAIL_CALL_SENTINEL);
    // Both sentinels must be distinct from a nil JitValue.
    assert_ne!(YIELD_SENTINEL, JitValue::nil());
    assert_ne!(TAIL_CALL_SENTINEL, JitValue::nil());
}
