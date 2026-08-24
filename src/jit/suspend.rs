//! Yield side-exit helpers for JIT-compiled code

use super::dispatch::YIELD_SENTINEL;
use crate::jit::value::JitValue;
use crate::value::{BytecodeFrame, SuspendedFrame, Value};

// =============================================================================
// Yield Side-Exit Helpers
// =============================================================================

/// `--trace=park`: log a JIT side-exit park with its frame shape. The resume
/// twin lives in `resume_suspended`; together they show whether a wrong value
/// was parked wrong or went wrong while parked.
fn trace_park(
    helper: &str,
    closure: &crate::value::Closure,
    site_index: u64,
    resume_ip: usize,
    env: &[Value],
    stack: &[Value],
) {
    if !crate::config::get().has_trace("park") {
        return;
    }
    eprintln!(
        "[park:{helper}] fn={} site={site_index} resume_ip={resume_ip} \
         env[{}]=[{}] stack[{}]=[{}]",
        closure.template.name.as_deref().unwrap_or("<anonymous>"),
        env.len(),
        Value::type_name_line(env),
        stack.len(),
        Value::type_name_line(stack),
    );
}

/// Park-time tripwire (debug builds): every value the side-exit parks must be
/// structurally sound (`Value::malformed_reason`). A torn or zeroed slot here
/// means the compiled frame's spill diverged from the interpreter layout the
/// resume expects; detonating at the park names the function and the slot
/// instead of leaving a segfault for the resumed reader.
#[cfg(debug_assertions)]
fn check_parked_frame(
    helper: &str,
    closure: &crate::value::Closure,
    resume_ip: usize,
    env: &[Value],
    stack: &[Value],
) {
    let name = closure.template.name.as_deref().unwrap_or("<anonymous>");
    for (section, values) in [("env", env), ("stack", stack)] {
        for (i, v) in values.iter().enumerate() {
            if let Some(reason) = v.malformed_reason() {
                panic!(
                    "{helper}: parked a malformed value — {reason}: \
                     fn={name} resume_ip={resume_ip} {section}[{i}] \
                     tag=0x{:x} payload=0x{:x} (env_len={} stack_len={} \
                     num_params={} num_locals={})",
                    v.tag,
                    v.payload,
                    env.len(),
                    stack.len(),
                    closure.template.num_params,
                    closure.template.num_locals,
                );
            }
        }
    }
}

#[cfg(not(debug_assertions))]
fn check_parked_frame(
    _helper: &str,
    _closure: &crate::value::Closure,
    _resume_ip: usize,
    _env: &[Value],
    _stack: &[Value],
) {
}

/// JIT yield side-exit: build a SuspendedFrame and set fiber.signal.
///
/// Called from JIT code when a Yield terminator is reached.
///
/// Parameters:
///   yielded_tag/yielded_payload: the value being yielded
///   spilled_values: *const Value (16 bytes each), or null if nothing to spill
///   yield_index: index into JitCode.yield_points
///   vm: *mut () (raw VM pointer)
///   closure_tag/closure_payload: the closure being executed (for self-tail-call detection)
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield(
    yielded_tag: u64,
    yielded_payload: u64,
    spilled_values: *const Value,
    yield_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
    signal_bits: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let yielded = Value {
        tag: yielded_tag,
        payload: yielded_payload,
    };
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield called with non-closure self");

    // Look up yield point metadata from JitCode
    let bytecode_ptr = closure.template.bytecode.as_ptr();
    let jit_code = vm
        .jit_cache
        .get(&bytecode_ptr)
        .expect("VM bug: elle_jit_yield called but no JitCode in cache");
    let yield_meta = &jit_code.yield_points[yield_index as usize];
    let num_params = yield_meta.num_params as usize;
    let num_locals = yield_meta.num_locals as usize;
    let num_operands = yield_meta.num_spilled as usize;

    // Spill buffer layout: [params(num_params), locals(num_locals), operands(num_spilled)]
    //
    // The interpreter expects:
    //   env = [captures, params]         ← LoadUpvalue reads from here
    //   stack = [locals, operands]       ← LoadLocal reads from here
    //
    // LBox cells are first-class values in JIT registers (no auto-unwrap),
    // so spilled cells are the original objects — no re-wrapping needed.
    let num_captures = closure.env.len();
    let mut env = Vec::with_capacity(num_captures + num_params);
    env.extend(closure.env.iter().copied());
    for i in 0..num_params {
        env.push(unsafe { *spilled_values.add(i) });
    }
    let env = std::rc::Rc::new(env);

    let mut stack = Vec::with_capacity(num_locals + num_operands);
    for i in num_params..(num_params + num_locals + num_operands) {
        stack.push(unsafe { *spilled_values.add(i) });
    }

    // Escape retain for the emitted value — the exact mirror of the
    // interpreter's `Emit` handler (`handle_emit`, src/vm/dispatch.rs). The
    // yielded value escapes into `fiber.signal`, where the resumer reads it via
    // `fiber/value`. The compiler emits a `DecrefRegion` at the emit's
    // decref_point (fired as this activation suspends and, on resume, continues
    // past the yield); without this incref that decref drops the value's only
    // reference while the resumer still holds it, freeing it out from under the
    // read (tests/elle/region-jit-emit-escape-uaf.lisp). The symmetric release
    // is the resume path's own pending decref, `release_discarded_signal` for a
    // fiber that never runs again, or the free-path fiber discharge — all
    // tier-agnostic (they act on `fiber.signal`), so they balance this retain
    // exactly as they balance `handle_emit`'s. `region_of` no-ops an immediate.
    let sig = crate::value::fiber::SignalBits::new(signal_bits);
    {
        let heap = unsafe { &mut *vm.heap_ptr };
        let yielded_region = crate::value::arena::region_of(heap, yielded);
        crate::value::arena::incref_for_escape(
            heap,
            yielded_region,
            crate::value::arena::EscapeSite::EmitEscape,
        );
    }
    vm.fiber.signal = Some((sig, yielded));

    if !sig.intersects(crate::value::fiber::SIG_ERROR) {
        // Suspension: build a frame for later resumption. The compiled
        // prologue pushed THIS activation's region-remap frame
        // (`elle_jit_push_region_map`), and the side-exit's pop runs after
        // this helper, so `last()` is this activation's map — captured here so
        // post-resume allocs/decrefs (re-entering via the interpreter) resolve
        // in the same frame the pre-yield allocations did.
        let activation_region_map = vm
            .fiber
            .activation_region_maps
            .last()
            .cloned()
            .unwrap_or_default();
        // Extract `code`/`resume_ip` into locals first: this ends the closure and
        // jit-cache borrows so the `&mut self` VM accessors below are free to run.
        let code = closure.template.code();
        let resume_ip = yield_meta.resume_ip;
        trace_park("jit-yield", closure, yield_index, resume_ip, &env, &stack);
        check_parked_frame("elle_jit_yield", closure, resume_ip, &env, &stack);
        // MOVE the activation's owner node into the frame (its slot is
        // likewise still on top) so it rides the park to the resumed body's
        // completion — the compiled twin of the interpreter yield park
        // (docs/impl/region/owner.md § "Owner nodes").
        let activation_owner_node = vm.take_activation_owner_node();
        // The yielding body's own closure — park it so a self-edge resolved after
        // resume (re-entering via the interpreter) names the right closure. The JIT
        // already threads it here for self-tail-call detection.
        let frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
            code,
            env,
            resume_ip,
            stack,
            true,
            activation_region_map,
            activation_owner_node,
            closure_val,
            vm.heap(),
        ));
        vm.fiber.suspended = Some(vec![frame]);
    }

    YIELD_SENTINEL
}

/// JIT yield-through-call: append a caller frame to fiber.suspended.
///
/// Called from JIT code when a callee yields (detected by post-call
/// signal check). Builds a caller SuspendedFrame and appends it to
/// the existing suspended frame chain.
///
/// Parameters:
///   spilled_values: *const Value (16 bytes each)
///   call_site_index: index into JitCode.call_sites
///   vm: *mut () as u64
///   closure_tag/closure_payload: the closure being executed
///
/// Returns YIELD_SENTINEL.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous `Value`s
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield_through_call(
    spilled_values: *const Value,
    call_site_index: u64,
    vm: u64, // *mut () as u64
    closure_tag: u64,
    closure_payload: u64,
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let closure_val = Value {
        tag: closure_tag,
        payload: closure_payload,
    };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield_through_call called with non-closure");

    // Look up call site metadata from JitCode
    let bytecode_ptr = closure.template.bytecode.as_ptr();
    let jit_code = vm
        .jit_cache
        .get(&bytecode_ptr)
        .expect("VM bug: elle_jit_yield_through_call called but no JitCode in cache");
    let call_meta = &jit_code.call_sites[call_site_index as usize];

    let num_params = call_meta.num_params as usize;
    let num_locals = call_meta.num_locals as usize;
    let num_operands = call_meta.num_spilled as usize;

    // Spill buffer layout: [params(num_params), locals(num_locals), operands(num_spilled)]
    //
    // The interpreter expects:
    //   env = [captures, params]         ← LoadUpvalue reads from here
    //   stack = [locals, operands]       ← LoadLocal reads from here
    //
    // LBox cells are first-class values in JIT registers (no auto-unwrap),
    // so spilled cells are the original objects — no re-wrapping needed.
    let num_captures = closure.env.len();
    let mut env = Vec::with_capacity(num_captures + num_params);
    env.extend(closure.env.iter().copied());
    for i in 0..num_params {
        env.push(unsafe { *spilled_values.add(i) });
    }
    let env = std::rc::Rc::new(env);

    let mut stack = Vec::with_capacity(num_locals + num_operands);
    for i in num_params..(num_params + num_locals + num_operands) {
        stack.push(unsafe { *spilled_values.add(i) });
    }

    // JIT caller frame: on resume, the callee's return value flows as
    // current_value and must be pushed as the Call instruction's result. The
    // suspended callee already popped its own region-map frame, so `last()` is
    // THIS caller's map (pushed by its compiled prologue) — captured for the
    // interpreter resume (see elle_jit_yield).
    let activation_region_map = vm
        .fiber
        .activation_region_maps
        .last()
        .cloned()
        .unwrap_or_default();
    // Extract `code`/`resume_ip` into locals first: this ends the closure and
    // jit-cache borrows so the `&mut self` VM accessors below are free to run.
    let code = closure.template.code();
    let resume_ip = call_meta.resume_ip;
    trace_park(
        "jit-yield-through-call",
        closure,
        call_site_index,
        resume_ip,
        &env,
        &stack,
    );
    check_parked_frame(
        "elle_jit_yield_through_call",
        closure,
        resume_ip,
        &env,
        &stack,
    );
    // MOVE the caller's owner node into its park — this compiled activation
    // unwinds with the callee's suspending signal (see elle_jit_yield).
    let activation_owner_node = vm.take_activation_owner_node();
    // The caller body's own closure (see elle_jit_yield) — park it for the resume.
    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
        code,
        env,
        resume_ip,
        stack,
        true,
        activation_region_map,
        activation_owner_node,
        closure_val,
        vm.heap(),
    ));

    // Append caller frame to the existing suspended chain.
    let mut frames = vm.fiber.suspended.take().unwrap_or_default();
    frames.push(caller_frame);
    vm.fiber.suspended = Some(frames);

    YIELD_SENTINEL
}

/// Check if any non-OK signal is pending on the VM.
/// Returns TRUE if set, FALSE otherwise.
///
/// This extends `elle_jit_has_exception` to also detect suspending signals
/// (SIG_YIELD, SIG_SWITCH, user-defined). Used after Call instructions in
/// yielding functions.
///
/// Checks `!is_empty()` rather than matching specific signal bits, because
/// I/O primitives return compound signals like `SIG_YIELD | SIG_IO` and
/// SIG_SWITCH must also be detected for fiber/resume trampolining.
#[no_mangle]
pub extern "C" fn elle_jit_has_signal(vm: u64) -> JitValue {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    JitValue::bool_val(vm.fiber.signal.as_ref().is_some_and(|(b, _)| !b.is_empty()))
}

#[cfg(test)]
mod tests;
