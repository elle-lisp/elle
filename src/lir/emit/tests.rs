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

// ── Merge-block operand depth ──
//
// A block inherits its stack simulation from the first predecessor that
// reaches it, so that predecessor fixes the merge's operand depth and every
// later edge must arrive at the same depth (src/lir/AGENTS.md § "Merge operand
// depth"). `pop_trailing_orphans_to` is the only lever the emitter has, and it
// runs on `Terminator::Jump` only — so a jump edge that trims past the depth a
// sibling branch edge left behind desynchronizes the two paths, and the
// orphan the jump already removed is popped a second time downstream.

/// A two-diamond function that pins the rule. The entry block leaves ONE
/// orphan on the operand stack, then branches; `L1` is the diamond's other
/// arm and jumps to the merge. The merge branches again, and both of its arms
/// jump to the exit.
///
/// Slot 2 holds the sentinel the function returns. It is the topmost local, so
/// it is the first casualty of a pop that falls through the reserved region:
/// the second (spurious) orphan pop shortens the stack to `num_locals - 1`, and
/// the exit block's `LoadLocal 2` then indexes past its end.
///
/// The orphan is built the way the lowerer builds one at a reassignment
/// (`lower_assign`'s drop-on-overwrite arm): push the new value, push the old
/// value, store the new value — `ensure_on_top` must `DupN` it back to the top
/// past the old one — then consume the old value. What remains is the new
/// value's original cell, which no register names any more.
fn orphan_across_merge_func() -> LirFunction {
    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_locals = 3;
    func.num_regs = 6;
    func.entry = Label(0);

    let konst = |dst: Reg, n: i64| {
        SpannedInstr::new(
            LirInstr::Const {
                dst,
                value: LirConst::Int(n),
            },
            synthetic_span(),
        )
    };
    let store = |slot: u16, src: Reg| {
        SpannedInstr::new(LirInstr::StoreLocal { slot, src }, synthetic_span())
    };
    let load = |dst: Reg, slot: u16| {
        SpannedInstr::new(LirInstr::LoadLocal { dst, slot }, synthetic_span())
    };
    let jump = |label: Label| SpannedTerminator::new(Terminator::Jump(label), synthetic_span());

    // Entry: park the sentinel in slot 2, then manufacture the orphan.
    let mut entry = BasicBlock::new(Label(0));
    entry.instructions.push(konst(Reg(0), 42));
    entry.instructions.push(store(2, Reg(0)));
    entry.instructions.push(konst(Reg(1), 7)); // the "new" value
    entry.instructions.push(konst(Reg(2), 9)); // the "old" value, pushed above it
    entry.instructions.push(store(0, Reg(1))); // DupN past the old value, then store
    entry.instructions.push(store(1, Reg(2))); // consume the old value
                                               // Simulated stack is now [Reg(1)] — one orphan, and it is on top.
    entry.instructions.push(load(Reg(3), 0));
    entry.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(3),
            then_label: Label(1),
            else_label: Label(2),
        },
        synthetic_span(),
    );
    func.blocks.push(entry);

    // The diamond's other arm: nothing but the jump to the merge.
    let mut arm = BasicBlock::new(Label(1));
    arm.terminator = jump(Label(2));
    func.blocks.push(arm);

    // The merge, which branches again into a second diamond.
    let mut merge = BasicBlock::new(Label(2));
    merge.instructions.push(load(Reg(4), 1));
    merge.terminator = SpannedTerminator::new(
        Terminator::Branch {
            cond: Reg(4),
            then_label: Label(3),
            else_label: Label(4),
        },
        synthetic_span(),
    );
    func.blocks.push(merge);

    for label in [Label(3), Label(4)] {
        let mut b = BasicBlock::new(label);
        b.terminator = jump(Label(5));
        func.blocks.push(b);
    }

    // Exit: read the sentinel back out of the topmost local and return it.
    let mut exit = BasicBlock::new(Label(5));
    exit.instructions.push(load(Reg(5), 2));
    exit.terminator = SpannedTerminator::new(Terminator::Return(Reg(5)), synthetic_span());
    func.blocks.push(exit);

    func
}

#[test]
fn merge_predecessors_leave_equal_operand_depth() {
    // Locals 0 and 1 hold 7 and 9, so both branches take their `then` arm and
    // the run passes through two jump edges. Each edge may drop the orphan at
    // most once between them; a second drop shortens the stack into the
    // reserved local region and the sentinel in slot 2 stops existing.
    let func = orphan_across_merge_func();
    let mut emitter = Emitter::new();
    let (bytecode, _, _) = emitter.emit(&func);
    let mut vm = crate::vm::VM::new();
    let result = vm.execute(&bytecode);
    assert_eq!(
        result.ok().and_then(|v| v.as_int()),
        Some(42),
        "a jump edge into an already-fixed merge must not pop below the depth \
         the branch edge left, or the frame loses its topmost local"
    );
}

// ── The coalescing equivalence oracle (`AssertRegionMatches`) ──
//
// `AssertRegionMatches { region_id, src }` is the debug-only net under
// coalescing: it panics in the bytecode interpreter when a static region slot
// resolves (through the activation map) to a different physical region than the
// value actually lives in — turning a mis-coalesce (a UAF in waiting) into a
// deterministic panic at the exact instruction. These pins prove the net both
// *bites* (wrong slot → panic) and is *precise* (right slot → silent), built
// from the spec in `LirInstr::AssertRegionMatches`, not from emission output.

/// A one-block function that allocates a fresh pair in `alloc_slot`, then runs
/// the oracle against `assert_slot` on that pair, then returns it. When the two
/// slots match, the oracle's resolve equals `region_of(pair)`; when they differ,
/// `assert_slot` is unmapped (never allocated this activation) and resolves to
/// `None`, which the pair's real region contradicts.
fn oracle_probe_func(alloc_slot: u32, assert_slot: u32) -> LirFunction {
    use crate::hir::region::StaticRegion;
    let s_alloc = StaticRegion::new(alloc_slot).expect("alloc slot nonzero");
    let s_assert = StaticRegion::new(assert_slot).expect("assert slot nonzero");

    let mut func = LirFunction::new(Arity::Exact(0));
    func.num_regs = 3;
    let mut block = BasicBlock::new(Label(0));
    // r0 ← nil (pair head), r1 ← () (pair tail).
    block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Nil,
        },
        synthetic_span(),
    ));
    block.instructions.push(SpannedInstr::new(
        LirInstr::Const {
            dst: Reg(1),
            value: LirConst::EmptyList,
        },
        synthetic_span(),
    ));
    // r2 ← pair(r0, r1), born in `s_alloc` (records slot→phys in the activation map).
    block.instructions.push(SpannedInstr::new(
        LirInstr::List {
            dst: Reg(2),
            head: Reg(0),
            tail: Reg(1),
            region: s_alloc,
        },
        synthetic_span(),
    ));
    // The oracle: assert `s_assert` names r2's physical region.
    block.instructions.push(SpannedInstr::new(
        LirInstr::AssertRegionMatches {
            region_id: s_assert,
            src: Reg(2),
        },
        synthetic_span(),
    ));
    block.terminator = SpannedTerminator::new(Terminator::Return(Reg(2)), synthetic_span());
    func.blocks.push(block);
    func.entry = Label(0);
    func
}

#[test]
fn assert_region_matches_passes_on_correct_slot() {
    // The pair is allocated in slot 1 and the oracle checks slot 1: the slot
    // resolves to exactly the pair's physical region, so the oracle is silent
    // and the function returns the pair. (Precision half — the net must not
    // false-positive on a genuinely coincident slot, which is every coalesced
    // site.)
    let func = oracle_probe_func(1, 1);
    let mut emitter = Emitter::new();
    let (bytecode, _, _) = emitter.emit(&func);
    let mut vm = crate::vm::VM::new();
    let result = vm.execute(&bytecode);
    assert!(
        result.is_ok(),
        "the coalescing oracle must stay silent when the slot names the value's \
         own region; got {result:?}"
    );
}

#[test]
#[should_panic(expected = "AssertRegionMatches")]
fn assert_region_matches_panics_on_wrong_slot() {
    // The pair is allocated in slot 1 but the oracle checks slot 2, which this
    // activation never allocated: it resolves to `None`, contradicting the
    // pair's real region. A coalescer that mapped this return to slot 2 would be
    // mis-coalescing — the oracle must detonate deterministically here, not let
    // the later cascade free a live region (a UAF). Counterfactual: with the
    // handler's check absent (release / no-op), this returns normally and the
    // test fails — proving the net is load-bearing.
    let func = oracle_probe_func(1, 2);
    let mut emitter = Emitter::new();
    let (bytecode, _, _) = emitter.emit(&func);
    let mut vm = crate::vm::VM::new();
    let _ = vm.execute(&bytecode);
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
