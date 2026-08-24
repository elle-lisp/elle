// Re-exported (`pub use`) so the per-theme submodules below can reach these
// names through their own `use super::*;` — a private `use` glob is not visible
// to child modules, so the test bodies (kept verbatim) would not otherwise
// resolve `Value`, `SuspendedFrame`, `elle_jit_yield`, `YieldPointMeta`, etc.
use super::super::dispatch::YieldPointMeta;
use super::*;
use crate::value::fiber::{SignalBits, SIG_YIELD};

// =========================================================================
// JIT yield: SuspendedFrame layout invariant
// =========================================================================

/// Set up a VM + Closure + JitCode for yield tests.
/// Returns (vm, closure_value) with the JitCode already in jit_cache.
fn setup_yield_test(
    bytecode: Vec<u8>,
    constants: Vec<Value>,
    env: Vec<Value>,
    yield_points: Vec<YieldPointMeta>,
) -> (crate::vm::VM, Value) {
    use crate::signals::Signal;
    use crate::value::types::Arity;
    use crate::value::ClosureTemplate;

    use std::rc::Rc;
    use std::sync::Arc;

    let bytecode = Rc::new(bytecode);
    let constants = Rc::new(constants);

    let template = Rc::new(ClosureTemplate {
        signal: Signal::yields(),
        ..ClosureTemplate::new(bytecode.clone(), Arity::Exact(0), constants)
    });

    // VM must exist before allocating the closure env slice; it owns the heap.
    let mut vm = crate::vm::VM::new();

    // Build the env slice and the closure header in ONE explicit region (slice
    // + header share that region, named through the ctx) on the VM's own heap —
    // the same heap the closure is allocated into below.
    let region = unsafe { (*vm.heap_ptr).new_runtime_region() };
    let env_slice = crate::value::arena::alloc_region_slice_in_region::<Value>(
        unsafe { &mut *vm.heap_ptr },
        &env,
        region,
    );
    let closure = crate::value::Closure {
        template: crate::value::TemplateRef::new(template.clone()),
        env: env_slice,
        squelch_mask: SignalBits::EMPTY,
    };

    // The closure header must share `region` with its env slice (see above), so
    // allocate it into `region` explicitly via a NativeCtx over this VM rather
    // than dropping the region argument.
    let closure_val = crate::primitives::ctx::NativeCtx::with_region_vm(
        region,
        unsafe { &mut *vm.heap_ptr },
        &mut vm as *mut crate::vm::VM,
    )
    .closure(closure);

    let jit_code = Arc::new(crate::jit::JitCode::test_with_yield_points(yield_points));
    vm.install_jit_code(bytecode, jit_code);

    (vm, closure_val)
}

/// Set up a VM + Closure + JitCode with LBox masks for yield tests.
fn setup_yield_test_with_lbox(
    bytecode: Vec<u8>,
    constants: Vec<Value>,
    env: Vec<Value>,
    yield_points: Vec<YieldPointMeta>,
    num_params: usize,
    capture_params_mask: u64,
    capture_locals_mask: u64,
) -> (crate::vm::VM, Value) {
    use crate::signals::Signal;
    use crate::value::types::Arity;
    use crate::value::ClosureTemplate;

    use std::rc::Rc;
    use std::sync::Arc;

    let bytecode = Rc::new(bytecode);
    let constants = Rc::new(constants);

    let template = Rc::new(ClosureTemplate {
        num_params,
        signal: Signal::yields(),
        capture_params_mask,
        capture_locals_mask: crate::value::CaptureMask::from_u64(capture_locals_mask),
        ..ClosureTemplate::new(bytecode.clone(), Arity::Exact(num_params), constants)
    });

    // VM must exist before allocating the closure env slice; it owns the heap.
    let mut vm = crate::vm::VM::new();

    // Build the env slice and the closure header in ONE explicit region (slice
    // + header share that region, named through the ctx) on the VM's own heap —
    // the same heap the closure is allocated into below.
    let region = unsafe { (*vm.heap_ptr).new_runtime_region() };
    let env_slice = crate::value::arena::alloc_region_slice_in_region::<Value>(
        unsafe { &mut *vm.heap_ptr },
        &env,
        region,
    );
    let closure = crate::value::Closure {
        template: crate::value::TemplateRef::new(template.clone()),
        env: env_slice,
        squelch_mask: SignalBits::EMPTY,
    };

    // The closure header must share `region` with its env slice (see above), so
    // allocate it into `region` explicitly via a NativeCtx over this VM rather
    // than dropping the region argument.
    let closure_val = crate::primitives::ctx::NativeCtx::with_region_vm(
        region,
        unsafe { &mut *vm.heap_ptr },
        &mut vm as *mut crate::vm::VM,
    )
    .closure(closure);

    let jit_code = Arc::new(crate::jit::JitCode::test_with_yield_points(yield_points));
    vm.install_jit_code(bytecode, jit_code);

    (vm, closure_val)
}

/// Extract the BytecodeFrame from a SuspendedFrame::Bytecode variant.
fn as_bytecode_frame(frame: &SuspendedFrame) -> &BytecodeFrame {
    match frame {
        SuspendedFrame::Bytecode(f) => f,
        _ => panic!("expected SuspendedFrame::Bytecode"),
    }
}

mod layout;
mod park;
mod values;
