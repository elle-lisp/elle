// audited: 2026-10-06
// docs/impl/bytecode.md
//! What the bytecode emitter writes: control flow, yield points, the
//! coalescing oracle, and a nested lambda's payload. What an edge owes the
//! operand stack is `depth`'s subject, and where a call parks is `callsite`'s.

use super::*;

mod callsite;
mod depth;
mod opcodes;
use crate::lir::testkit::LirFixture;
use crate::value::{Arity, CodeArena};

/// An emitter over a code region of a heap the test leaks, for the tests that
/// read what the emitter wrote and run none of it.
pub(super) fn emitter() -> Emitter {
    Emitter::new(CodeArena::mint(unsafe {
        &mut *crate::value::arena::leaked_test_heap()
    }))
}

/// Emit `func` alone into a code region of `vm`'s heap and run it there.
pub(super) fn run_on(
    vm: &mut crate::vm::VM,
    func: &LirOwned,
) -> Result<crate::value::Value, String> {
    let code = CodeArena::mint(vm.heap());
    let (bytecode, _, _) = Emitter::new(code).emit(&func.view());
    vm.execute(&crate::value::CodeUnit::new(code, bytecode))
}

#[test]
fn test_emit_simple() {
    let mut emitter = emitter();

    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Int(42),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let (bytecode, _, _) = emitter.emit(&func.view());
    assert!(!bytecode.instructions.is_empty());
}

#[test]
fn test_emit_branch() {
    let mut emitter = emitter();

    let func = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Bool(true),
            }],
            Terminator::Branch {
                cond: Reg(0),
                then_label: Label(1),
                else_label: Label(2),
            },
        )
        .block(
            1,
            &[InstrRef::Const {
                dst: Reg(1),
                value: ConstRef::Int(1),
            }],
            Terminator::Return(Reg(1)),
        )
        .block(
            2,
            &[InstrRef::Const {
                dst: Reg(2),
                value: ConstRef::Int(2),
            }],
            Terminator::Return(Reg(2)),
        )
        .build();

    let (bytecode, _, _) = emitter.emit(&func.view());
    assert!(!bytecode.instructions.is_empty());
    // Should have Jump instructions for control flow
    assert!(bytecode
        .instructions
        .iter()
        .any(|&b| b == Instruction::Jump as u8 || b == Instruction::JumpIfFalse as u8));
}

#[test]
fn test_yield_point_info_collected() {
    let mut emitter = emitter();

    // fn() { yield 42; resume_value }
    let func = LirFixture::new(Arity::Exact(0))
        .signal(crate::signals::Signal::yields())
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Int(42),
            }],
            Terminator::Emit {
                signal: crate::value::fiber::SIG_YIELD,
                value: Reg(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            &[InstrRef::LoadResumeValue { dst: Reg(1) }],
            Terminator::Return(Reg(1)),
        )
        .build();

    let (bytecode, yield_points, _call_sites) = emitter.emit(&func.view());
    assert!(!bytecode.instructions.is_empty());
    assert_eq!(yield_points.len(), 1);
    assert!(yield_points[0].resume_ip > 0);
    // stack_regs should be empty — only Reg(0) was on stack, but it was
    // popped by the Yield. The remaining stack is empty.
    assert!(yield_points[0].stack_regs.is_empty());
}

// ── The coalescing equivalence oracle (`AssertRegionMatches`) ──
//
// `AssertRegionMatches { region_id, src }` is the debug-only net under
// coalescing: it panics in the bytecode interpreter when a static region slot
// resolves (through the activation map) to a different physical region than the
// value actually lives in — turning a mis-coalesce (a UAF in waiting) into a
// deterministic panic at the exact instruction. These pins prove the net both
// *bites* (wrong slot → panic) and is *precise* (right slot → silent), built
// from the spec in `InstrRef::AssertRegionMatches`, not from emission output.

/// A one-block function that allocates a fresh pair in `alloc_slot`, then runs
/// the oracle against `assert_slot` on that pair, then returns it. When the two
/// slots match, the oracle's resolve equals `region_of(pair)`; when they differ,
/// `assert_slot` is unmapped (never allocated this activation) and resolves to
/// `None`, which the pair's real region contradicts.
fn oracle_probe_func(alloc_slot: u32, assert_slot: u32) -> LirOwned {
    use crate::hir::region::StaticRegion;
    let s_alloc = StaticRegion::new(alloc_slot).expect("alloc slot nonzero");
    let s_assert = StaticRegion::new(assert_slot).expect("assert slot nonzero");

    LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[
                // r0 ← nil (pair head), r1 ← () (pair tail).
                InstrRef::Const {
                    dst: Reg(0),
                    value: ConstRef::Nil,
                },
                InstrRef::Const {
                    dst: Reg(1),
                    value: ConstRef::EmptyList,
                },
                // r2 ← pair(r0, r1), born in `s_alloc` (records slot→phys in the
                // activation map).
                InstrRef::List {
                    dst: Reg(2),
                    head: Reg(0),
                    tail: Reg(1),
                    region: s_alloc,
                },
                // The oracle: assert `s_assert` names r2's physical region.
                InstrRef::AssertRegionMatches {
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
    let mut vm = crate::vm::VM::new();
    let result = run_on(&mut vm, &func);
    assert!(
        result.is_ok(),
        "the coalescing oracle must stay silent when the slot names the value's \
         own region; got {result:?}"
    );
}

// The interpreter's handler wraps the check in `#[cfg(debug_assertions)]`, so
// this is the profile the oracle exists in.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "AssertRegionMatches")]
fn assert_region_matches_panics_on_wrong_slot() {
    // The pair is allocated in slot 1 but the oracle checks slot 2, which this
    // activation never allocated: it resolves to `None`, contradicting the
    // pair's real region. A coalescer that mapped this return to slot 2 would be
    // mis-coalescing — the oracle must detonate deterministically here, not let
    // the later cascade free a live region (a UAF). Counter-factual: with the
    // handler's check reduced to a no-op, this returns normally, so the
    // assertion is what catches the mis-coalesce.
    let func = oracle_probe_func(1, 2);
    let mut vm = crate::vm::VM::new();
    let _ = run_on(&mut vm, &func);
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
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Int(42),
            }],
            Terminator::Emit {
                signal,
                value: Reg(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            &[InstrRef::LoadResumeValue { dst: Reg(1) }],
            Terminator::Return(Reg(1)),
        )
        .build();

    let (bytecode, _, _) = emitter().emit(&func.view());
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

/// A nested lambda's payload carries both halves of the abandoned-frame
/// release table, so a closure the emitted `MakeClosure` builds reaches an
/// error exit with the releases it still owes (docs/impl/region/template.md
/// § "One constructor builds a nested lambda's payload").
#[test]
fn a_nested_lambdas_payload_carries_the_frame_release_tables() {
    // Counter-factual: a payload built with both tables empty fails nothing
    // that runs. The closure built over it carries real bytecode and returns
    // the right answers; what it loses is one error exit's walk, which
    // strands every region the abandoned frame still owed.
    use crate::hir::region::StaticRegion;
    use crate::lir::ClosureId;

    let nested = LirFixture::new(Arity::Exact(0))
        .name("nested")
        .head(|h| {
            h.frame_release_slots = vec![3, 7];
            h.frame_release_regions = vec![
                StaticRegion::new(11).unwrap(),
                StaticRegion::new(13).unwrap(),
            ];
        })
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let outer = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: &[],
                region: StaticRegion::new(2).unwrap(),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let module = FrozenModule {
        entry: outer,
        closures: vec![nested],
    };
    let code = CodeArena::mint(unsafe { &mut *crate::value::arena::leaked_test_heap() });
    let (bytecode, _, _) = Emitter::new(code).emit_module(&module);
    let unit = crate::value::CodeUnit::new(code, bytecode);
    assert_eq!(
        unit.entry().num_children(),
        1,
        "one MakeClosure registers one child"
    );
    let child = unit.entry().child(0);
    assert_eq!(
        child.frame_release_slots(),
        &[3u16, 7],
        "the value route's slots reach the payload",
    );
    assert_eq!(
        child.frame_release_regions(),
        &[11u32, 13],
        "the slot route's regions reach the payload",
    );
}

/// A nested lambda's payload carries the rest-list layout the gate wrote onto
/// its `LirHead`, so a closure the emitted `MakeClosure` builds builds its rest
/// list the way the analysis proved it may (docs/impl/region/restlist.md).
#[test]
fn a_nested_lambdas_payload_carries_its_rest_list_layout() {
    // Counter-factual: a payload left at the default layout runs correctly and
    // claims a page per rest argument, which no answer the closure returns can
    // show.
    use crate::hir::region::StaticRegion;
    use crate::lir::ClosureId;
    use crate::value::RestListLayout;

    let nested = LirFixture::new(Arity::AtLeast(0))
        .name("nested")
        .num_params(1)
        .num_locals(1)
        .head(|h| h.rest_list_layout = RestListLayout::OneRegion)
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let outer = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: &[],
                region: StaticRegion::new(2).unwrap(),
            }],
            Terminator::Return(Reg(0)),
        )
        .build();

    let module = FrozenModule {
        entry: outer,
        closures: vec![nested],
    };
    let code = CodeArena::mint(unsafe { &mut *crate::value::arena::leaked_test_heap() });
    let (bytecode, _, _) = Emitter::new(code).emit_module(&module);
    let unit = crate::value::CodeUnit::new(code, bytecode);
    assert_eq!(
        unit.entry().child(0).rest_list_layout(),
        RestListLayout::OneRegion,
        "the gate's verdict reaches the payload"
    );
}
