//! Emit-size invariants for the CPS suspend/resume machinery.
//!
//! A suspending WASM function spills live state before a yield/suspending-call
//! and restores it on resume. The code for this must scale with the *live* set
//! at each suspend point, not with `suspend_points × total_slots`. When it
//! doesn't, a large suspending stdlib function (e.g. a string builder that
//! happens to be marked `may_suspend`) emits a multi-megabyte function body
//! that exceeds Wasmtime's per-function size limit and the whole module fails
//! to parse. These tests pin the linear-in-slots invariant.

use super::emit::emit_module;
use crate::lir::{
    BasicBlock, Label, LirConst, LirFunction, LirInstr, LirModule, Reg, SpannedInstr,
    SpannedTerminator, Terminator,
};
use crate::signals::{Signal, SIG_YIELD};
use crate::syntax::Span;
use crate::value::Arity;

fn spanned(instr: LirInstr) -> SpannedInstr {
    SpannedInstr::new(instr, Span::synthetic())
}

fn block(label: u32, instrs: Vec<LirInstr>, term: Terminator) -> BasicBlock {
    let mut b = BasicBlock::new(Label(label));
    b.instructions = instrs.into_iter().map(spanned).collect();
    b.terminator = SpannedTerminator::new(term, Span::synthetic());
    b
}

/// A trivial non-suspending entry that just returns nil, so the module has a
/// valid entry function alongside the closure under test.
fn trivial_entry() -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(0));
    f.num_regs = 1;
    f.blocks = vec![block(
        0,
        vec![LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Nil,
        }],
        Terminator::Return(Reg(0)),
    )];
    f
}

/// A suspending closure with `n_yields` yield points and `n_locals` declared
/// local slots that are never read (dead). Value register `Reg(0)` is defined
/// once and carried across every yield to the final return, so it stays live
/// throughout. Because the locals are dead, a live-aware emitter spills none of
/// them; a slot-count-blind emitter spills and restores all of them at every
/// one of the `n_yields` points.
fn suspending_closure(n_yields: u32, n_locals: u16) -> LirFunction {
    let mut f = LirFunction::new(Arity::Exact(1));
    f.closure_id = Some(crate::lir::ClosureId(0));
    f.num_regs = 1;
    f.num_locals = n_locals;
    f.num_params = 1;
    f.signal = Signal::yields();

    let mut blocks = Vec::new();
    // Block 0 defines the carried value, then yields to block 1.
    blocks.push(block(
        0,
        vec![LirInstr::Const {
            dst: Reg(0),
            value: LirConst::Int(1),
        }],
        Terminator::Emit {
            signal: SIG_YIELD,
            value: Reg(0),
            resume_label: Label(1),
        },
    ));
    // Middle yield blocks, each carrying Reg(0) forward.
    for i in 1..n_yields {
        blocks.push(block(
            i,
            vec![],
            Terminator::Emit {
                signal: SIG_YIELD,
                value: Reg(0),
                resume_label: Label(i + 1),
            },
        ));
    }
    // Final block returns the carried value.
    blocks.push(block(n_yields, vec![], Terminator::Return(Reg(0))));
    f.blocks = blocks;
    f
}

/// Emit a module whose single closure is `func`, returning the module bytes.
fn emit_bytes(func: LirFunction) -> Vec<u8> {
    let vm = crate::vm::VM::new();
    let module = LirModule {
        entry: trivial_entry(),
        closures: vec![func],
    };
    emit_module(&module, std::collections::HashSet::new(), vm.heap_ptr).wasm_bytes
}

#[test]
fn resume_and_spill_are_linear_in_slots_not_states_times_slots() {
    // 80 yield points, 150 dead local slots. With per-state dense restore and
    // dense local spill the body is ~O(states × slots) ≈ hundreds of KB; with
    // hoisted restore and live-aware local spill it is ~O(states + slots).
    let bytes = emit_bytes(suspending_closure(80, 150));

    // Generous ceiling: the linear emission is a few KB of dispatch + one
    // restore of the declared slots. The quadratic emission is ~280KB. 40KB
    // sits well above the linear size and well below the quadratic one.
    assert!(
        bytes.len() < 40_000,
        "suspending closure emitted {} bytes — spill/restore is scaling with \
         states × slots instead of live slots (CPS size blowup)",
        bytes.len()
    );
}

#[test]
fn dead_locals_do_not_multiply_by_state_count() {
    // The core invariant: adding local slots that are dead across every suspend
    // point must add O(slots) to the module (they are declared and reloaded once
    // by the hoisted restore), never O(states × slots) (spilled AND restored at
    // every one of the suspend points). Hold the state count fixed and grow the
    // dead-local count by 500.
    let few = emit_bytes(suspending_closure(100, 20)).len();
    let many = emit_bytes(suspending_closure(100, 520)).len();
    // 500 extra dead locals across 100 states. Live-aware, hoisted emission adds
    // a few KB (one restore of the extra slots + their declarations); the
    // states × slots emission would add ~100 × 500 slots of spill + restore,
    // on the order of a megabyte.
    assert!(
        many - few < 60_000,
        "500 extra dead locals grew the module by {} bytes at 100 states — \
         spill/restore is scaling with states × slots, not live slots",
        many - few
    );
}
