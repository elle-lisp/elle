// audited: 2026-09-06
// docs/impl/wasm.md
//! The WASM backend's tests, one file per subject, over the LIR fixtures and
//! evaluation helpers they share.

use super::emit::{emit_module, emit_single_closure};
use crate::lir::testkit::LirFixture;
use crate::lir::{ClosureId, Label, LirConst, LirFunction, LirInstr, LirModule, Reg, Terminator};
use crate::signals::{Signal, SIG_YIELD};
use crate::value::Arity;

mod cache;
mod closure;
mod collections;
mod emitsize;
mod errors;
mod fibers;
mod frame;
mod gate;
mod gauge;
mod stdlib;
mod toplevel;

/// A trivial non-suspending entry that just returns nil, so the module has a
/// valid entry function alongside the closure under test.
fn trivial_entry() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// A suspending closure with `n_yields` yield points and `n_locals` declared
/// local slots that are never read (dead). Value register `Reg(0)` is defined
/// once and carried across every yield to the final return, so it stays live
/// throughout. Because the locals are dead, a live-aware emitter spills none of
/// them; a slot-count-blind emitter spills and restores all of them at every
/// one of the `n_yields` points.
fn suspending_closure(n_yields: u32, n_locals: u16) -> LirFunction {
    let mut f = LirFixture::new(Arity::Exact(1))
        .closure_id(ClosureId(0))
        .num_locals(n_locals)
        .num_params(1)
        .signal(Signal::yields())
        // Block 0 defines the carried value, then yields to block 1.
        .block(
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
        );
    // Middle yield blocks, each carrying Reg(0) forward.
    for i in 1..n_yields {
        f = f.block(
            i,
            vec![],
            Terminator::Emit {
                signal: SIG_YIELD,
                value: Reg(0),
                resume_label: Label(i + 1),
            },
        );
    }
    // Final block returns the carried value.
    f.block(n_yields, vec![], Terminator::Return(Reg(0)))
        .build()
}

/// Emit a module whose single closure is `func`, returning the module bytes.
fn emit_bytes(func: LirFunction) -> Vec<u8> {
    let vm = crate::vm::VM::new();
    let module = LirModule {
        entry: trivial_entry(),
        closures: vec![func],
    };
    emit_module(
        &module,
        std::collections::HashSet::new(),
        vm.heap_ptr,
        std::ptr::null_mut(),
    )
    .wasm_bytes
}

fn static_region(id: u32) -> crate::hir::region::StaticRegion {
    crate::hir::region::StaticRegion::new(id).expect("nonzero static slot")
}

/// Run stdlib-backed source through the full-module WASM backend and return the
/// result's display form. `eval_wasm_*` materializes that string while its
/// per-call heap is alive, so a compound result (list, string, mutable) is
/// returned safely — not left dangling past the heap's teardown.
fn eval_with_stdlib(source: &str) -> String {
    match super::eval_wasm_with_stdlib(source, "<macro-stdlib>") {
        Ok(s) => s,
        Err(e) => panic!("eval_wasm_with_stdlib failed: {}", e),
    }
}

/// Run stdlib-free source through the full-module WASM backend and return the
/// result's display form (materialized while the heap is alive; see
/// [`eval_with_stdlib`]).
fn eval(source: &str) -> String {
    match super::eval_wasm(source, "<gauge>") {
        Ok(s) => s,
        Err(e) => panic!("eval_wasm failed: {}", e),
    }
}
