//! Runtime dispatch helpers for JIT-compiled code
//!
//! These functions handle complex operations that interact with heap types
//! or require VM access: data structures, cells, globals, and function calls.
//!
//! Data structure/cell helpers are in `data.rs`; yield helpers in `suspend.rs`.
//! Re-exported here so `compiler.rs` can reference them as `dispatch::*`.

use crate::value::fiber::{
    SignalBits, SIG_CANCEL, SIG_ERROR, SIG_HALT, SIG_OK, SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
    SIG_YIELD,
};
use crate::value::repr::TAG_NIL;
use crate::value::{error_val, Value};

// Re-export split modules so compiler.rs can still use dispatch::elle_jit_*
pub use super::data::*;
pub use super::suspend::*;

// =============================================================================
// Primitive Signal Handling (for JIT dispatch)
// =============================================================================

/// Handle signal bits from a primitive call in JIT context.
///
/// With the relaxed JIT gate, SIG_YIELD can now appear here from primitives
/// like `fiber/resume`. VM-internal signals (SIG_RESUME, SIG_PROPAGATE,
/// SIG_CANCEL) are dispatched to the VM's fiber handlers, which run the
/// child fiber synchronously and return a result.
/// SIG_ERROR sets the exception on the fiber for the JIT caller to check.
/// SIG_QUERY is dispatched to the VM's query handler (for primitives like
/// `list-primitives` and `primitive-meta` that read VM state).
fn jit_handle_primitive_signal(vm: &mut crate::vm::VM, bits: SignalBits, value: Value) -> u64 {
    match bits {
        SIG_OK => value.to_bits(),
        SIG_ERROR | SIG_HALT => {
            vm.fiber.signal = Some((bits, value));
            TAG_NIL
        }
        SIG_QUERY => {
            // arena/allocs needs mutable VM access to call the thunk —
            // handle before dispatch_query (which takes &self).
            if let Some(cons) = value.as_cons() {
                if cons.first.as_keyword_name() == Some("arena/allocs") {
                    let thunk = cons.rest;
                    match vm.handle_arena_allocs(thunk) {
                        Ok(val) => return val.to_bits(),
                        Err(_bits) => return TAG_NIL,
                    }
                }
            }
            // Dispatch VM state query and return the result.
            let (sig, result) = vm.dispatch_query(value);
            if sig == SIG_ERROR {
                vm.fiber.signal = Some((SIG_ERROR, result));
                TAG_NIL
            } else {
                result.to_bits()
            }
        }
        SIG_YIELD => {
            // A primitive yielded (e.g., fiber/resume). fiber.signal is
            // already set by the primitive. Return YIELD_SENTINEL so the
            // JIT caller can side-exit.
            YIELD_SENTINEL
        }
        SIG_RESUME => {
            // Fiber primitive (fiber/resume, coro/resume) returned
            // SIG_RESUME. Dispatch to the VM's fiber handler which runs
            // the child fiber synchronously and returns value bits,
            // TAG_NIL (error), or YIELD_SENTINEL (yield propagation).
            vm.handle_fiber_resume_signal_jit(value)
        }
        SIG_PROPAGATE => {
            // fiber/propagate: propagate the child fiber's signal.
            vm.handle_fiber_propagate_signal_jit(value)
        }
        SIG_CANCEL => {
            // fiber/cancel: inject error into suspended fiber.
            vm.handle_fiber_cancel_signal_jit(value)
        }
        _ => {
            panic!(
                "Unhandled signal {} reached JIT-compiled code. \
                 This indicates a missing signal handler in jit_handle_primitive_signal.",
                bits
            );
        }
    }
}

// =============================================================================
// Tail Call Support
// =============================================================================

/// Sentinel value indicating a pending tail call.
/// Using a specific bit pattern that can't be a valid Value.
/// The VM checks for this after call_jit returns.
pub const TAIL_CALL_SENTINEL: u64 = 0xDEAD_BEEF_DEAD_BEEFu64;

/// Sentinel value indicating a JIT function yielded (side-exited).
/// The caller checks for this after a JIT call and propagates the yield.
/// fiber.signal and fiber.suspended are already set by the JIT yield helper.
pub const YIELD_SENTINEL: u64 = 0xDEAD_CAFE_DEAD_CAFEu64;

/// Metadata for a single yield point in JIT-compiled code.
/// Stored in `JitCode.yield_points`, indexed by yield point index.
/// Read by `elle_jit_yield` runtime helper (Chunk 2).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct YieldPointMeta {
    /// Bytecode IP to resume at (matches the interpreter's SuspendedFrame.ip)
    pub resume_ip: usize,
    /// Number of spilled values that constitute the operand stack.
    /// Single source of truth — the JIT yield helper reads this, not a parameter.
    pub num_spilled: u16,
    /// Number of local variable slots (params + locally-defined).
    /// The JIT spills locals first, then operand stack registers.
    /// The runtime helper uses this to split the spilled buffer into
    /// locals and operands, matching the interpreter's stack layout:
    /// `[local_0, ..., local_{n-1}, operand_0, ..., operand_m]`.
    pub num_locals: u16,
}

/// Metadata for a single call site in JIT-compiled code.
/// Stored in `JitCode.call_sites`, indexed by call site index.
/// Read by `elle_jit_yield_through_call` runtime helper.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct CallSiteMeta {
    /// Bytecode IP to resume at (matches the interpreter's SuspendedFrame.ip)
    pub resume_ip: usize,
    /// Total number of spilled values (locals + operands).
    pub num_spilled: u16,
    /// Number of local variable slots (params + locally-defined).
    pub num_locals: u16,
}

// =============================================================================
// Exception Checking
// =============================================================================

/// Check if a terminal signal is pending on the VM (error or halt).
/// Returns TRUE bits if one is set, FALSE bits otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_has_exception(vm: u64) -> u64 {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    Value::bool(matches!(vm.fiber.signal, Some((SIG_ERROR | SIG_HALT, _)))).to_bits()
}

// =============================================================================
// Function Calls
// =============================================================================

/// Reinterpret a JIT args pointer as a `&[Value]` slice.
///
/// Safe because `Value` is `#[repr(transparent)]` over `u64`, so `*const u64`
/// and `*const Value` have identical layout. Handles the null-pointer case
/// when `nargs` is 0 (JIT may pass null for zero-arg calls).
#[inline]
fn args_ptr_to_value_slice(args_ptr: *const u64, nargs: u32) -> &'static [Value] {
    if nargs == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr as *const Value, nargs as usize) }
    }
}

/// Call a function from JIT code.
///
/// Dispatches to native functions or closures. When the callee has
/// JIT-compiled code in the cache, calls it directly (JIT-to-JIT)
/// without building an interpreter environment — zero heap allocations
/// on the fast path.
#[no_mangle]
pub extern "C" fn elle_jit_call(
    func_bits: u64,
    args_ptr: *const u64,
    nargs: u32,
    vm: *mut (),
) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = unsafe { Value::from_bits(func_bits) };

    // Dispatch to native function — zero-copy args via repr(transparent)
    if let Some(f) = func.as_native_fn() {
        let args_slice = args_ptr_to_value_slice(args_ptr, nargs);
        let (bits, value) = f(args_slice);
        return jit_handle_primitive_signal(vm, bits, value);
    }

    // Dispatch to parameter (dynamic binding lookup)
    if let Some((id, default)) = func.as_parameter() {
        if nargs != 0 {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", nargs),
                ),
            ));
            return TAG_NIL;
        }
        return vm.resolve_parameter(id, default).to_bits();
    }

    // Dispatch to closure
    if let Some(closure) = func.as_closure() {
        // Check arity using nargs directly — no Vec needed
        if !vm.check_arity(&closure.template.arity, nargs as usize) {
            return TAG_NIL;
        }

        // JIT-to-JIT fast path: check if callee has JIT code
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        if let Some(jit_code) = vm.jit_cache.get(&bytecode_ptr).cloned() {
            vm.fiber.call_depth += 1;
            if vm.fiber.call_depth > 1000 {
                vm.fiber.signal = Some((SIG_ERROR, error_val("error", "Stack overflow")));
                vm.fiber.call_depth -= 1;
                return TAG_NIL;
            }

            // Zero-copy env pointer: Value is #[repr(transparent)] over u64,
            // so closure.env's &[Value] can be reinterpreted as *const u64.
            let env_ptr = if closure.env.is_empty() {
                std::ptr::null()
            } else {
                closure.env.as_ptr() as *const u64
            };

            // Args pass through directly — already *const u64 from JIT caller
            let result_bits = unsafe {
                jit_code.call(
                    env_ptr,
                    args_ptr,
                    nargs,
                    vm as *mut crate::vm::VM as *mut (),
                    func_bits,
                )
            };

            vm.fiber.call_depth -= 1;

            // Check for exception (error or halt)
            if matches!(vm.fiber.signal, Some((SIG_ERROR | SIG_HALT, _))) {
                return TAG_NIL;
            }

            // Check for yield from callee
            if matches!(vm.fiber.signal, Some((SIG_YIELD, _))) {
                return YIELD_SENTINEL;
            }

            // Handle tail call sentinel
            if result_bits == TAIL_CALL_SENTINEL {
                if let Some(tail) = vm.pending_tail_call.take() {
                    let result = vm.execute_bytecode_saving_stack(
                        &tail.bytecode,
                        &tail.constants,
                        &tail.env,
                        &tail.location_map,
                    );
                    return exec_result_to_jit_bits(vm, result.bits);
                }
            }

            // Check for YIELD_SENTINEL from callee (defensive — signal check
            // above should catch this first, but the callee might have returned
            // YIELD_SENTINEL without setting fiber.signal in a bug scenario)
            if result_bits == YIELD_SENTINEL {
                return YIELD_SENTINEL;
            }

            return result_bits;
        }

        // Interpreter fallback — reconstruct args Vec for env building
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { Value::from_bits(*args_ptr.add(i)) })
            .collect();

        let new_env = build_closure_env_for_jit(closure, &args);

        vm.fiber.call_depth += 1;
        let result = vm.execute_bytecode_saving_stack(
            &closure.template.bytecode,
            &closure.template.constants,
            &new_env,
            &closure.template.location_map,
        );
        vm.fiber.call_depth -= 1;

        exec_result_to_jit_bits(vm, result.bits)
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val("type-error", format!("Cannot call {:?}", func)),
        ));
        TAG_NIL
    }
}

/// Resolve a pending tail call after a direct SCC call.
///
/// When a directly-called SCC peer returns TAIL_CALL_SENTINEL (because it
/// tail-called something outside the SCC), the caller must resolve it.
/// This helper checks for the sentinel and executes the pending tail call.
///
/// Returns the final result value, or TAG_NIL if an error occurred.
#[no_mangle]
pub extern "C" fn elle_jit_resolve_tail_call(result: u64, vm: *mut ()) -> u64 {
    if result != TAIL_CALL_SENTINEL {
        return result;
    }
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    if let Some(tail) = vm.pending_tail_call.take() {
        let result = vm.execute_bytecode_saving_stack(
            &tail.bytecode,
            &tail.constants,
            &tail.env,
            &tail.location_map,
        );
        exec_result_to_jit_bits(vm, result.bits)
    } else {
        panic!(
            "VM bug: TAIL_CALL_SENTINEL returned but no pending_tail_call set. \
             This indicates a bug in the JIT tail call protocol."
        );
    }
}

/// Increment call depth and check for stack overflow.
///
/// Used by direct SCC calls (which bypass `elle_jit_call` and its built-in
/// depth tracking). Returns 0 (falsy) on success, or non-zero (truthy) if
/// the call depth exceeds 1000 (after setting the error signal on the fiber).
#[no_mangle]
pub extern "C" fn elle_jit_call_depth_enter(vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.call_depth += 1;
    if vm.fiber.call_depth > 1000 {
        vm.fiber.signal = Some((SIG_ERROR, error_val("error", "Stack overflow")));
        vm.fiber.call_depth -= 1;
        return 1; // truthy — overflow
    }
    0 // falsy — ok
}

/// Decrement call depth after a direct SCC call returns.
///
/// Pairs with `elle_jit_call_depth_enter`. Always returns TAG_NIL (ignored
/// by callers — this is a void-like helper).
#[no_mangle]
pub extern "C" fn elle_jit_call_depth_exit(vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.call_depth -= 1;
    TAG_NIL
}

/// Convert an ExecResult from execute_bytecode_saving_stack to a JIT return value.
/// Handles SIG_OK, SIG_HALT (both return the value), SIG_YIELD (returns
/// YIELD_SENTINEL), and errors (signal already set, returns TAG_NIL).
fn exec_result_to_jit_bits(vm: &mut crate::vm::VM, bits: SignalBits) -> u64 {
    match bits {
        SIG_OK | SIG_HALT => {
            let (_, val) = vm.fiber.signal.take().unwrap();
            val.to_bits()
        }
        SIG_YIELD => YIELD_SENTINEL,
        _ => {
            // SIG_ERROR — signal already set on fiber
            TAG_NIL
        }
    }
}

/// Handle a non-self tail call from JIT code.
///
/// If the target closure has JIT code in the cache, calls it directly —
/// avoiding the round-trip through the interpreter. This is critical for
/// mutual recursion (e.g., `solve-helper` tail-calling `try-cols-helper`):
/// without this, every cross-function tail call drops from JIT to the
/// interpreter dispatch loop.
///
/// If the callee returns TAIL_CALL_SENTINEL (its own non-self tail call),
/// we propagate it to our caller for resolution — the pending_tail_call
/// env is in interpreter format and can't be used for another JIT call.
///
/// Falls back to TAIL_CALL_SENTINEL (interpreter trampoline) only when
/// the target has no JIT code.
#[no_mangle]
pub extern "C" fn elle_jit_tail_call(
    func_bits: u64,
    args_ptr: *const u64,
    nargs: u32,
    vm: *mut (),
) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = unsafe { Value::from_bits(func_bits) };

    // Handle native functions — zero-copy args via repr(transparent)
    if let Some(f) = func.as_native_fn() {
        let args_slice = args_ptr_to_value_slice(args_ptr, nargs);
        let (bits, value) = f(args_slice);
        return jit_handle_primitive_signal(vm, bits, value);
    }

    // Handle parameter (dynamic binding lookup)
    if let Some((id, default)) = func.as_parameter() {
        if nargs != 0 {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val(
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", nargs),
                ),
            ));
            return TAG_NIL;
        }
        return vm.resolve_parameter(id, default).to_bits();
    }

    // Handle closures
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.template.arity, nargs as usize) {
            return TAG_NIL;
        }

        // JIT fast path: if the target has JIT code, call it directly
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        if let Some(jit_code) = vm.jit_cache.get(&bytecode_ptr).cloned() {
            let env_ptr = if closure.env.is_empty() {
                std::ptr::null()
            } else {
                closure.env.as_ptr() as *const u64
            };

            let result_bits = unsafe {
                jit_code.call(
                    env_ptr,
                    args_ptr,
                    nargs,
                    vm as *mut crate::vm::VM as *mut (),
                    func_bits,
                )
            };

            // Check for exception
            if matches!(vm.fiber.signal, Some((SIG_ERROR | SIG_HALT, _))) {
                return TAG_NIL;
            }

            // Check for yield from callee
            if matches!(vm.fiber.signal, Some((SIG_YIELD, _))) {
                return YIELD_SENTINEL;
            }

            if result_bits == YIELD_SENTINEL {
                return YIELD_SENTINEL;
            }

            // Propagate result (including TAIL_CALL_SENTINEL) to caller
            return result_bits;
        }

        // Interpreter fallback — build env, return TAIL_CALL_SENTINEL
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { Value::from_bits(*args_ptr.add(i)) })
            .collect();

        let new_env = build_closure_env_for_jit(closure, &args);
        vm.pending_tail_call = Some(crate::vm::core::TailCallInfo {
            bytecode: closure.template.bytecode.clone(),
            constants: closure.template.constants.clone(),
            env: new_env,
            location_map: closure.template.location_map.clone(),
        });

        return TAIL_CALL_SENTINEL;
    }

    vm.fiber.signal = Some((
        SIG_ERROR,
        error_val("type-error", format!("Cannot call {:?}", func)),
    ));
    TAG_NIL
}

// =============================================================================
// Environment Building
// =============================================================================

/// Push a parameter value into the environment buffer, wrapping in a
/// LocalCell if the lbox_params_mask indicates it's needed.
#[inline]
fn push_param(buf: &mut Vec<Value>, closure: &crate::value::Closure, i: usize, val: Value) {
    if i < 64 && (closure.template.lbox_params_mask & (1 << i)) != 0 {
        buf.push(Value::local_lbox(val));
    } else {
        buf.push(val);
    }
}

/// Collect values into an Elle list (cons chain terminated by EMPTY_LIST).
fn args_to_list(args: &[Value]) -> Value {
    let mut list = Value::EMPTY_LIST;
    for arg in args.iter().rev() {
        list = Value::cons(*arg, list);
    }
    list
}

/// Build a closure environment from captured variables and arguments.
///
/// Mirrors `VM::populate_env` for the interpreter fallback path in JIT
/// dispatch. Handles all arity variants including variadic (`AtLeast`)
/// with rest-arg collection and `Range` with optional parameters.
fn build_closure_env_for_jit(
    closure: &crate::value::Closure,
    args: &[Value],
) -> std::rc::Rc<Vec<Value>> {
    let mut new_env = Vec::with_capacity(closure.env_capacity());
    new_env.extend((*closure.env).iter().cloned());

    match closure.template.arity {
        crate::value::Arity::Exact(_) => {
            for (i, arg) in args.iter().enumerate() {
                push_param(&mut new_env, closure, i, *arg);
            }
        }
        crate::value::Arity::AtLeast(_) => {
            // Total fixed slots = num_params - 1 (rest slot is last param)
            let fixed_slots = closure.template.num_params - 1;

            // Determine how many positional args to consume for fixed slots.
            // For &keys/&named, keyword args should not fill optional slots.
            let collects_keywords = matches!(
                closure.template.vararg_kind,
                crate::hir::VarargKind::Struct | crate::hir::VarargKind::StrictStruct(_)
            );
            let provided_fixed = if collects_keywords {
                let min = closure.template.arity.fixed_params();
                let mut count = args.len().min(min);
                while count < fixed_slots && count < args.len() {
                    if args[count].as_keyword_name().is_some() {
                        break;
                    }
                    count += 1;
                }
                count
            } else {
                args.len().min(fixed_slots)
            };

            // Push args for fixed slots (required + optional)
            for (i, arg) in args[..provided_fixed].iter().enumerate() {
                push_param(&mut new_env, closure, i, *arg);
            }
            // Fill missing optional slots with nil
            for i in provided_fixed..fixed_slots {
                push_param(&mut new_env, closure, i, Value::NIL);
            }

            // Collect remaining args into rest slot.
            // Note: Struct/StrictStruct vararg kinds require fiber access for
            // error reporting, which is unavailable in the JIT dispatch context.
            // Only List collection is supported here. Struct varargs are rare
            // in JIT-eligible code (they require keyword argument parsing).
            let rest_args = if args.len() > provided_fixed {
                &args[provided_fixed..]
            } else {
                &[]
            };
            let collected = args_to_list(rest_args);
            push_param(&mut new_env, closure, fixed_slots, collected);
        }
        crate::value::Arity::Range(_, max) => {
            // All slots are fixed (no rest param)
            for (i, arg) in args.iter().enumerate() {
                push_param(&mut new_env, closure, i, *arg);
            }
            // Fill missing optional slots with nil
            for i in args.len()..max {
                push_param(&mut new_env, closure, i, Value::NIL);
            }
        }
    }

    // Calculate number of locally-defined variables
    let num_params = match closure.template.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n,
        crate::value::Arity::Range(min, _) => min,
    };
    let num_locally_defined = closure.template.num_locals.saturating_sub(num_params);

    // Add slots for locally-defined variables.
    // cell-wrapped locals get LocalCell(NIL); non-cell locals get bare NIL.
    // Beyond index 63, conservatively use LocalCell.
    for i in 0..num_locally_defined {
        if i >= 64 || (closure.template.lbox_locals_mask & (1 << i)) != 0 {
            new_env.push(Value::local_lbox(Value::NIL));
        } else {
            new_env.push(Value::NIL);
        }
    }

    std::rc::Rc::new(new_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_exception() {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;
        use crate::vm::VM;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // Initially no exception
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(false));

        // Set an error signal
        vm.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val("division-by-zero", "test"),
        ));

        // Now should return true
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(false));
    }
}
