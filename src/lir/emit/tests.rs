//! Tests for LIR to bytecode emission

use super::*;
use crate::lir::testkit::LirFixture;
use crate::value::Arity;

#[test]
fn test_emit_simple() {
    let mut emitter = Emitter::new();

    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let (bytecode, _, _) = emitter.emit(&func);
    assert!(!bytecode.instructions.is_empty());
}

#[test]
fn test_emit_branch() {
    let mut emitter = Emitter::new();

    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Bool(true),
            }],
            Terminator::Branch {
                cond: Reg(0),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(
            1,
            vec![LirInstr::Const {
                dst: Reg(1),
                value: LirConst::Int(1),
            }],
            Terminator::Return(Reg(1)),
        )
        .block(
            2,
            vec![LirInstr::Const {
                dst: Reg(2),
                value: LirConst::Int(2),
            }],
            Terminator::Return(Reg(2)),
        )
        .build();

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
    let func = LirFixture::new(Arity::Exact(0))
        .signal(crate::signals::Signal::yields())
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            }],
            Terminator::Emit {
                signal: crate::value::fiber::SIG_YIELD,
                value: Reg(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            vec![LirInstr::LoadResumeValue { dst: Reg(1) }],
            Terminator::Return(Reg(1)),
        )
        .build();

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
    let konst = |dst: Reg, n: i64| LirInstr::Const {
        dst,
        value: LirConst::Int(n),
    };
    let store = |slot: u16, src: Reg| LirInstr::StoreLocal { slot, src };
    let load = |dst: Reg, slot: u16| LirInstr::LoadLocal { dst, slot };

    let mut func = LirFixture::new(Arity::Exact(0))
        .num_locals(3)
        // Entry: park the sentinel in slot 2, then manufacture the orphan.
        .block(
            0,
            vec![
                konst(Reg(0), 42),
                store(2, Reg(0)),
                konst(Reg(1), 7), // the "new" value
                konst(Reg(2), 9), // the "old" value, pushed above it
                store(0, Reg(1)), // DupN past the old value, then store
                store(1, Reg(2)), // consume the old value
                load(Reg(3), 0),  // stack is now [Reg(1)] — one orphan, on top
            ],
            Terminator::Branch {
                cond: Reg(3),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        // The diamond's other arm: nothing but the jump to the merge.
        .block(1, vec![], Terminator::Jump(Label(2)))
        // The merge, which branches again into a second diamond.
        .block(
            2,
            vec![load(Reg(4), 1)],
            Terminator::Branch {
                cond: Reg(4),
                then_label: Label(3),
                else_label: Label(4),
            },
        );

    for label in [3, 4] {
        func = func.block(label, vec![], Terminator::Jump(Label(5)));
    }

    // Exit: read the sentinel back out of the topmost local and return it.
    func.block(5, vec![load(Reg(5), 2)], Terminator::Return(Reg(5)))
        .build()
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

    LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![
                // r0 ← nil (pair head), r1 ← () (pair tail).
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Nil,
                },
                LirInstr::Const {
                    dst: Reg(1),
                    value: LirConst::EmptyList,
                },
                // r2 ← pair(r0, r1), born in `s_alloc` (records slot→phys in the
                // activation map).
                LirInstr::List {
                    dst: Reg(2),
                    head: Reg(0),
                    tail: Reg(1),
                    region: s_alloc,
                },
                // The oracle: assert `s_assert` names r2's physical region.
                LirInstr::AssertRegionMatches {
                    region_id: s_assert,
                    src: Reg(2),
                },
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
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

#[test]
fn emit_terminator_carries_a_user_signal_bit_whole() {
    // `(signal :keyword)` allocates bits 32-63, and the emitter bakes the mask
    // of a literal `emit` into the `Emit` operand. The whole mask must survive:
    // an emit of empty bits builds a suspension nothing can route, and the VM
    // tears the fiber down as if its body had returned.
    //
    // The trap: `:yield` and every other built-in sits in bits 0-15, so a
    // narrow operand carries them fine and only user signals disappear. This
    // asserts over a mask with a bit in each half for that reason.
    use crate::compiler::bytecode::disassemble_lines;
    use crate::value::fiber::SignalBits;

    let signal = SignalBits::from_bit(32).union(crate::value::fiber::SIG_YIELD);
    let func = LirFixture::new(Arity::Exact(0))
        .signal(crate::signals::Signal::of(signal))
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(42),
            }],
            Terminator::Emit {
                signal,
                value: Reg(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            vec![LirInstr::LoadResumeValue { dst: Reg(1) }],
            Terminator::Return(Reg(1)),
        )
        .build();

    let (bytecode, _, _) = Emitter::new().emit(&func);
    let lines = disassemble_lines(&bytecode.instructions);
    let emit_line = lines
        .iter()
        .find(|l| l.contains("Emit "))
        .unwrap_or_else(|| panic!("no Emit line in: {lines:?}"));
    assert!(
        emit_line.contains(&format!("signal_bits=0x{:016x}", signal.raw())),
        "got: {emit_line}"
    );
}

/// LIR `BinOp` names an operation, never an operand type, so it can only mean
/// the polymorphic bytecode. The integer-only family stays unemitted until a
/// typed LIR variant carries a proof of integer operands
/// (docs/impl/bytecode.md § Arithmetic).
#[test]
fn arithmetic_binops_emit_the_polymorphic_bytecodes() {
    // The trap: every integer opcode name contains its polymorphic one, so a
    // substring search for "Add" also matches an emitted `AddInt` and passes
    // under the very mapping it exists to reject. Compare the opcode token
    // whole.
    //
    // The counter-factual: without the negative half, mapping `BinOp::Add` to
    // `AddInt` still satisfies "some arithmetic opcode was emitted", and float
    // operands would silently take integer wrapping arithmetic.
    use crate::compiler::bytecode::disassemble_lines;

    for (op, polymorphic, integer) in [
        (BinOp::Add, "Add", "AddInt"),
        (BinOp::Sub, "Sub", "SubInt"),
        (BinOp::Mul, "Mul", "MulInt"),
        (BinOp::Div, "Div", "DivInt"),
    ] {
        let func = LirFixture::new(Arity::Exact(0))
            .block(
                0,
                vec![
                    LirInstr::Const {
                        dst: Reg(0),
                        value: LirConst::Int(6),
                    },
                    LirInstr::Const {
                        dst: Reg(1),
                        value: LirConst::Int(7),
                    },
                    LirInstr::BinOp {
                        dst: Reg(2),
                        op,
                        lhs: Reg(0),
                        rhs: Reg(1),
                    },
                ],
                Terminator::Return(Reg(2)),
            )
            .build();

        let (bytecode, _, _) = Emitter::new().emit(&func);
        let opcodes: Vec<String> = disassemble_lines(&bytecode.instructions)
            .iter()
            .filter_map(|line| line.split_once("] "))
            .map(|(_, rest)| rest.split(' ').next().unwrap_or_default().to_string())
            .collect();

        assert!(
            opcodes.iter().any(|name| name == polymorphic),
            "{polymorphic}: BinOp must emit the polymorphic opcode; got {opcodes:?}"
        );
        assert!(
            !opcodes.iter().any(|name| name == integer),
            "{integer}: BinOp must not emit the integer-only opcode; got {opcodes:?}"
        );
    }
}
