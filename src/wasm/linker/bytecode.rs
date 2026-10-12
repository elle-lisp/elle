// audited: 2026-10-06
//! Running a closure the module never compiled, on the host VM, and resuming
//! the chain it parked.
//!
//! docs/impl/wasm.md

use wasmtime::*;

use crate::value::fiber::ParamDepth;
use crate::value::{SignalBits, Value};
use crate::wasm::host::{BytecodePark, ElleHost, ParkedFrame};
use crate::wasm::outcome::CallOutcome;

/// Execute a **bytecode closure** — one with no `wasm_func_idx` — by calling
/// back into the host VM's interpreter, returning `(tag, payload, signal)` for
/// the wasm caller. The full-module twin of the tiered linker's bytecode-closure
/// path (`src/wasm/lazy/linker.rs`).
///
/// Only `stdlib.lisp` is compiled into the full module (see `wasm/mod.rs`
/// `eval_wasm_with_stdlib`); `core.lisp` and the prelude are loaded as
/// interpreter bytecode. So a compiled function — `map`'s list arm calling
/// `reverse`, a user HOF calling `last`/`fold`/`concat`, a callback bound at
/// runtime — routinely reaches a closure the module never compiled. Executing it
/// here via the VM makes the call transparent; without this the caller aborts
/// with "bytecode closure in WASM backend". Pinned by
/// `tests/lang/wasm-bytecode-closure-call.lisp`.
pub(in crate::wasm) fn run_bytecode_closure(
    caller: &mut Caller<'_, ElleHost>,
    closure: &crate::value::Closure,
    func_val: Value,
    args: &[Value],
) -> CallOutcome {
    let vm = caller.data().vm;
    // Raw-pointer deref: the VM outlives the call, and it is a distinct object
    // from the `ElleHost` reached via `caller.data_mut()` below (the value/handle
    // marshalling touches the host, never the VM), so the two never alias.
    let vm_ref = unsafe { &mut *vm };
    let Some(env) = vm_ref.build_closure_env(closure, args) else {
        // `build_closure_env` rejected the args (bad keywords) and set the error
        // signal; surface it.
        let (bits, val) = vm_ref
            .fiber
            .signal
            .unwrap_or((crate::value::SIG_ERROR, Value::NIL));
        let (tag, payload) = caller.data_mut().value_to_wasm(val);
        return CallOutcome::signalled(tag, payload, bits);
    };
    // Hand the callee its executing-closure register via the one-shot (the
    // WASM→interp entry boundary), so a self-reference in the body resolves to
    // the callee, not NIL — the same handoff the JIT's interpreter fallback and
    // the tiered fallback perform.
    vm_ref.pending_entry_closure = func_val;
    let depth = vm_ref.fiber.param_depth();
    let bits = vm_ref.run_thunk_to_completion(&closure.template.code(), &env);
    settle_bytecode_exit(caller, bits, depth)
}

/// Resume the front frame of the current fiber's chain when it is a bytecode
/// callee's chain, with `value` delivered into the call that parked it.
/// `None` when the front frame is a compiled closure's.
///
/// The chain resumes on the VM's fiber, where it parked: its `parameterize`
/// frames go back on, and the delivery owes the resume value the mint the
/// park recorded, as `handle_fiber_resume` does for a fiber
/// (docs/impl/region/park.md).
pub(in crate::wasm) fn resume_bytecode_park(
    caller: &mut Caller<'_, ElleHost>,
    value: Value,
) -> Option<CallOutcome> {
    let park = caller.data_mut().pop_bytecode_park()?;
    let vm = unsafe { &mut *caller.data().vm };
    if park.unfunded {
        let heap = unsafe { &mut *vm.heap_ptr };
        let r = crate::value::arena::region_of(heap, value);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::ResumeDelivery,
        );
    }
    let depth = vm.fiber.param_depth();
    vm.fiber.param_frames.extend(park.param_frames);
    let saved_closure = vm.fiber.current_closure;
    let bits = vm.resume_to_completion(park.frames, value);
    vm.fiber.current_closure = saved_closure;
    Some(settle_bytecode_exit(caller, bits, depth))
}

/// Turn the exit of bytecode the VM ran for a compiled caller into the call's
/// outcome. `depth` is the parameter depth where the bytecode was entered.
///
/// A park leaves the callee's chain on the VM's fiber, which is not the fiber
/// this tier drives, so the chain and what it owes move onto the host's chain
/// for the driven fiber, as its innermost frame. The compiled caller then parks
/// behind it. An empty chain parks nothing of the callee's: a restart answers
/// the compiled call, whose continuation takes no delivery mint.
fn settle_bytecode_exit(
    caller: &mut Caller<'_, ElleHost>,
    bits: SignalBits,
    depth: ParamDepth,
) -> CallOutcome {
    let vm = unsafe { &mut *caller.data().vm };
    let val = vm.fiber.signal.map_or(Value::NIL, |(_, v)| v);
    // A NIL `(halt)` is a normal return; every other terminal/suspending signal
    // rides back out on the signal word (mirrors `exec_result_to_jit_value`).
    let signal = if bits.is_empty() || (bits == crate::value::SIG_HALT && val == Value::NIL) {
        vm.fiber.signal.take();
        crate::value::SIG_OK
    } else {
        bits
    };
    if crate::signals::dispatch::parks(signal) {
        vm.fiber.signal.take();
        let unfunded = vm.fiber.delivery.take_resume_funding();
        let frames = vm.fiber.suspended.take().unwrap_or_default();
        let param_frames = vm.fiber.take_params_above(depth);
        if !frames.is_empty() {
            caller
                .data_mut()
                .push_parked(ParkedFrame::Bytecode(BytecodePark {
                    frames,
                    param_frames,
                    unfunded,
                    signal,
                }));
        }
    }
    let (tag, payload) = caller.data_mut().value_to_wasm(val);
    CallOutcome::signalled(tag, payload, signal)
}
