// audited: 2026-09-06
// docs/impl/wasm.md
//! What the standalone single-closure emission gate accepts and refuses.
//!
//! A standalone single-closure module is served by hosts whose suspension and
//! tail-call imports are panic stubs (src/wasm/lazy/env.rs) and whose funcref
//! table has one entry, so `emit_single_closure` must refuse every shape whose
//! execution would reach one of them (src/wasm/AGENTS.md § "Constraints on
//! per-closure compilation"). Refusal means `None`: the tiered caller falls
//! back to the bytecode VM, the precache caller to full-module dispatch.

use super::*;

/// A closure whose block carries one tail call (callee register is arbitrary —
/// the gate is structural, it never resolves the callee).
fn tail_calling_closure() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .closure_id(ClosureId(0))
        .num_params(1)
        .block(
            0,
            vec![
                LirInstr::Const {
                    dst: Reg(0),
                    value: LirConst::Int(1),
                },
                LirInstr::TailCall {
                    dst: Reg(1),
                    func: Reg(0),
                    args: vec![],
                    arity_checked: false,
                    region: static_region(2),
                    defer_callee_release: false,
                    deferred_release_slot: None,
                    borrowed_arg_slots: Vec::new(),
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .build()
}

/// A closure that constructs a nested closure (`MakeClosure`), resolvable only
/// with module context.
fn nested_closure_closure() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .closure_id(ClosureId(1))
        .num_params(1)
        .block(
            0,
            vec![LirInstr::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: vec![],
                region: static_region(2),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// A plain numeric closure — the positive control proving the gate is not
/// over-broad.
fn plain_closure() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .closure_id(ClosureId(0))
        .num_params(1)
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Int(7),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

#[test]
fn standalone_emission_admits_plain_closures() {
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(&plain_closure(), None, vm.heap_ptr, std::ptr::null_mut()).is_some(),
        "a numeric closure with no stub-reaching shape must be standalone-emittable"
    );
}

#[test]
fn standalone_emission_refuses_suspending_closures() {
    // An Emit terminator — yield and `(error …)` alike — routes through
    // rt_yield's suspension-frame machinery, a panic stub outside the
    // full-module store.
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(
            &suspending_closure(1, 0),
            None,
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_none(),
        "a closure with an Emit terminator compiled standalone would panic at \
         the tiered host's rt_yield stub"
    );
}

#[test]
fn standalone_emission_refuses_tail_calls() {
    // return_call_indirect needs callee funcref-table indices and
    // rt_prepare_tail_call — a panic stub, and a 1-entry table.
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(
            &tail_calling_closure(),
            None,
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_none(),
        "a closure with a TailCall compiled standalone would panic at the \
         tiered host's rt_prepare_tail_call stub"
    );
}

#[test]
fn standalone_emission_refuses_module_less_make_closure() {
    // ClosureId resolution needs the module's closure list; with it the shape
    // is admitted (the precache path), without it refused (the tiered path).
    let vm = crate::vm::VM::new();
    assert!(
        emit_single_closure(
            &nested_closure_closure(),
            None,
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_none(),
        "MakeClosure without module context has no ClosureId resolution"
    );
    let module = LirModule {
        entry: trivial_entry(),
        closures: vec![plain_closure(), nested_closure_closure()],
    };
    assert!(
        emit_single_closure(
            &nested_closure_closure(),
            Some(&module),
            vm.heap_ptr,
            std::ptr::null_mut()
        )
        .is_some(),
        "MakeClosure with module context resolves through rt_make_closure"
    );
}
