//! Function call dispatch helpers for JIT-compiled code.
//!
//! These `extern "C"` functions handle calling Elle closures, native functions,
//! and parameters from JIT-compiled code. They also include the sentinels,
//! yield/call-site metadata types, and the environment-building utility used
//! by the interpreter fallback paths.

use crate::jit::value::{JitValue, TAIL_CALL_SENTINEL_JV, YIELD_SENTINEL_JV};
use crate::value::fiber::{
    SignalBits, SIG_ABORT, SIG_ERROR, SIG_HALT, SIG_OK, SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
    SIG_YIELD,
};
use crate::value::{error_val, Value};

// =============================================================================
// Sentinels and Metadata Types
// =============================================================================

/// Sentinel `JitValue` indicating a pending tail call.
/// Uses a tag value that cannot be a valid Value tag (> TAG_THREAD = 33).
pub const TAIL_CALL_SENTINEL: JitValue = TAIL_CALL_SENTINEL_JV;

/// Sentinel `JitValue` indicating a JIT function yielded (side-exited).
/// The caller checks for this after a JIT call and propagates the yield.
/// fiber.signal and fiber.suspended are already set by the JIT yield helper.
pub const YIELD_SENTINEL: JitValue = YIELD_SENTINEL_JV;

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
// Primitive Signal Handling (for JIT dispatch)
// =============================================================================

/// Handle signal bits from a primitive call in JIT context.
///
/// Returns a `JitValue` for the result.
fn jit_handle_primitive_signal(vm: &mut crate::vm::VM, bits: SignalBits, value: Value) -> JitValue {
    if bits.is_ok() {
        return JitValue::from_value(value);
    }

    // --- VM-internal signals (exact match — never composed) ---

    if bits == SIG_RESUME {
        return vm.handle_fiber_resume_signal_jit(value);
    }

    if bits == SIG_PROPAGATE {
        return vm.handle_fiber_propagate_signal_jit(value);
    }

    if bits == SIG_ABORT && value.as_fiber().is_some() {
        return vm.handle_fiber_abort_signal_jit(value);
    }

    if bits == SIG_QUERY {
        if let Some(cons) = value.as_cons() {
            if cons.first.as_keyword_name().as_deref() == Some("arena/allocs") {
                let thunk = cons.rest;
                return match vm.handle_arena_allocs(thunk) {
                    Ok(val) => JitValue::from_value(val),
                    Err(_bits) => JitValue::nil(),
                };
            }
        }
        let (sig, result) = vm.dispatch_query(value);
        if sig == SIG_ERROR {
            vm.fiber.signal = Some((SIG_ERROR, result));
            return JitValue::nil();
        } else {
            return JitValue::from_value(result);
        }
    }

    // --- User-facing signals (contains — handles composed bits) ---

    if bits.contains(SIG_ERROR) {
        vm.fiber.signal = Some((bits, value));
        return JitValue::nil();
    }

    if bits.contains(SIG_HALT) {
        vm.fiber.signal = Some((bits, value));
        return JitValue::nil();
    }

    if bits.contains(SIG_YIELD) {
        vm.fiber.signal = Some((bits, value));
        return YIELD_SENTINEL;
    }

    // Any remaining signal: user-defined, SIG_DEBUG, SIG_FUEL, etc.
    vm.fiber.signal = Some((bits, value));
    YIELD_SENTINEL
}

// =============================================================================
// Exception Checking
// =============================================================================

/// Check if a terminal signal is pending on the VM (error or halt).
/// Returns TRUE if one is set, FALSE otherwise.
///
/// Uses bitwise containment (`contains`) rather than exact equality,
/// because signals can be compound (e.g. `SIG_ERROR | SIG_IO`).
#[no_mangle]
pub extern "C" fn elle_jit_has_exception(vm: u64) -> JitValue {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    JitValue::bool_val(
        vm.fiber
            .signal
            .as_ref()
            .is_some_and(|(b, _)| b.contains(SIG_ERROR) || b.contains(SIG_HALT)),
    )
}

// =============================================================================
// Function Calls
// =============================================================================

/// Reinterpret a JIT args pointer as a `&[Value]` slice.
///
/// The JIT passes a `*const Value` (16 bytes each). Handles the null-pointer
/// case when `nargs` is 0.
#[inline]
pub(crate) fn args_ptr_to_value_slice(args_ptr: *const Value, nargs: u32) -> &'static [Value] {
    if nargs == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args_ptr, nargs as usize) }
    }
}

/// Call a function from JIT code.
///
/// Dispatches to native functions or closures. When the callee has
/// JIT-compiled code in the cache, calls it directly (JIT-to-JIT)
/// without building an interpreter environment — zero heap allocations
/// on the fast path.
///
/// Parameters: func_tag/func_payload (the callee Value), args_ptr (*const Value),
/// nargs, vm.
/// Returns a `JitValue` for the result.
#[no_mangle]
pub extern "C" fn elle_jit_call(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = Value {
        tag: func_tag,
        payload: func_payload,
    };

    // Dispatch to native function — zero-copy args via *const Value
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
            return JitValue::nil();
        }
        let result = vm.resolve_parameter(id, default);
        return JitValue::from_value(result);
    }

    // Dispatch to closure
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.template.arity, nargs as usize) {
            return JitValue::nil();
        }

        // JIT-to-JIT fast path: check if callee has JIT code
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        if let Some(jit_code) = vm.jit_cache.get(&bytecode_ptr).cloned() {
            vm.fiber.call_depth += 1;
            if vm.fiber.call_depth > 1000 {
                vm.fiber.signal = Some((SIG_ERROR, error_val("stack-overflow", "Stack overflow")));
                vm.fiber.call_depth -= 1;
                return JitValue::nil();
            }

            let env_ptr = if closure.env.is_empty() {
                std::ptr::null()
            } else {
                closure.env.as_ptr()
            };

            let result = unsafe {
                jit_code.call(
                    env_ptr,
                    args_ptr,
                    nargs,
                    vm as *mut crate::vm::VM as *mut (),
                    func_tag,
                    func_payload,
                )
            };

            vm.fiber.call_depth -= 1;

            // Check for exception (error or halt) — use contains for compound signals
            if vm
                .fiber
                .signal
                .as_ref()
                .is_some_and(|(b, _)| b.contains(SIG_ERROR) || b.contains(SIG_HALT))
            {
                return JitValue::nil();
            }

            // Check for suspending signal from callee (SIG_YIELD, SIG_SWITCH, user-defined)
            if let Some((sig, _)) = vm.fiber.signal {
                if !sig.is_ok() && !sig.contains(SIG_ERROR) && !sig.contains(SIG_HALT) {
                    return YIELD_SENTINEL;
                }
            }

            // Handle tail call sentinel
            if result == TAIL_CALL_SENTINEL {
                if let Some(tail) = vm.pending_tail_call.take() {
                    let exec_result = vm.execute_bytecode_saving_stack(
                        &tail.bytecode,
                        &tail.constants,
                        &tail.env,
                        &tail.location_map,
                    );
                    return exec_result_to_jit_value(vm, exec_result.bits);
                }
            }

            // Defensive: if callee returned YIELD_SENTINEL without setting signal
            if result == YIELD_SENTINEL {
                return YIELD_SENTINEL;
            }

            return result;
        }

        // Interpreter fallback — reconstruct args Vec for env building
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { *args_ptr.add(i) })
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

        exec_result_to_jit_value(vm, result.bits)
    } else if let Some(result) =
        crate::vm::call::call_collection(&func, args_ptr_to_value_slice(args_ptr, nargs))
    {
        match result {
            Ok(value) => JitValue::from_value(value),
            Err((kind, msg)) => {
                vm.fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
                JitValue::nil()
            }
        }
    } else {
        vm.fiber.signal = Some((
            SIG_ERROR,
            error_val("type-error", format!("Cannot call {:?}", func)),
        ));
        JitValue::nil()
    }
}

/// Resolve a pending tail call after a direct SCC call.
///
/// When a directly-called SCC peer returns TAIL_CALL_SENTINEL (because it
/// tail-called something outside the SCC), the caller must resolve it.
/// This helper checks for the sentinel and executes the pending tail call.
///
/// Returns the final `JitValue`, or `JitValue::nil()` if an error occurred.
#[no_mangle]
pub extern "C" fn elle_jit_resolve_tail_call(
    result_tag: u64,
    result_payload: u64,
    vm: *mut (),
) -> JitValue {
    let result = JitValue {
        tag: result_tag,
        payload: result_payload,
    };
    if result != TAIL_CALL_SENTINEL {
        return result;
    }
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    if let Some(tail) = vm.pending_tail_call.take() {
        let exec_result = vm.execute_bytecode_saving_stack(
            &tail.bytecode,
            &tail.constants,
            &tail.env,
            &tail.location_map,
        );
        exec_result_to_jit_value(vm, exec_result.bits)
    } else {
        panic!(
            "VM bug: TAIL_CALL_SENTINEL returned but no pending_tail_call set. \
             This indicates a bug in the JIT tail call protocol."
        );
    }
}

/// Increment call depth and check for stack overflow.
///
/// Returns FALSE on success, or TRUE if the call depth exceeds 1000
/// (after setting the error signal on the fiber).
#[no_mangle]
pub extern "C" fn elle_jit_call_depth_enter(vm: *mut ()) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.call_depth += 1;
    if vm.fiber.call_depth > 1000 {
        vm.fiber.signal = Some((SIG_ERROR, error_val("stack-overflow", "Stack overflow")));
        vm.fiber.call_depth -= 1;
        return JitValue::bool_val(true); // truthy — overflow
    }
    JitValue::bool_val(false) // falsy — ok
}

/// Decrement call depth after a direct SCC call returns.
///
/// Pairs with `elle_jit_call_depth_enter`. Always returns NIL (ignored).
#[no_mangle]
pub extern "C" fn elle_jit_call_depth_exit(vm: *mut ()) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.call_depth -= 1;
    JitValue::nil()
}

/// Pop one dynamic parameter frame from the fiber.
/// Pairs with PushParamFrame. Returns NIL (ignored by caller).
#[no_mangle]
pub extern "C" fn elle_jit_pop_param_frame(vm: *mut ()) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    vm.fiber.param_frames.pop();
    JitValue::nil()
}

/// Call a function with arguments from an array value.
/// Unpacks the array and delegates to elle_jit_call.
#[no_mangle]
pub extern "C" fn elle_jit_call_array(
    func_tag: u64,
    func_payload: u64,
    args_array_tag: u64,
    args_array_payload: u64,
    vm: *mut (),
) -> JitValue {
    let args_val = Value {
        tag: args_array_tag,
        payload: args_array_payload,
    };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };

    let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(arr) = args_val.as_array() {
        arr.to_vec()
    } else {
        vm_ref.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            ),
        ));
        return JitValue::nil();
    };

    let nargs = args.len() as u32;
    if args.is_empty() {
        elle_jit_call(func_tag, func_payload, std::ptr::null(), nargs, vm)
    } else {
        elle_jit_call(func_tag, func_payload, args.as_ptr(), nargs, vm)
    }
}

/// Tail-call a function with arguments from an array value.
/// Unpacks the array and delegates to elle_jit_tail_call.
#[no_mangle]
pub extern "C" fn elle_jit_tail_call_array(
    func_tag: u64,
    func_payload: u64,
    args_array_tag: u64,
    args_array_payload: u64,
    vm: *mut (),
) -> JitValue {
    let args_val = Value {
        tag: args_array_tag,
        payload: args_array_payload,
    };
    let vm_ref = unsafe { &mut *(vm as *mut crate::vm::VM) };

    let args: Vec<Value> = if let Some(arr) = args_val.as_array_mut() {
        arr.borrow().to_vec()
    } else if let Some(arr) = args_val.as_array() {
        arr.to_vec()
    } else {
        vm_ref.fiber.signal = Some((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "splice: expected array or tuple for args, got {}",
                    args_val.type_name()
                ),
            ),
        ));
        return JitValue::nil();
    };

    let nargs = args.len() as u32;
    if args.is_empty() {
        elle_jit_tail_call(func_tag, func_payload, std::ptr::null(), nargs, vm)
    } else {
        elle_jit_tail_call(func_tag, func_payload, args.as_ptr(), nargs, vm)
    }
}

/// Create a closure from a template Value and captured environment.
/// template_tag/template_payload: the closure template Value
/// captures_ptr: pointer to array of `count` Values (16 bytes each)
/// count: number of captured values
#[no_mangle]
pub extern "C" fn elle_jit_make_closure(
    template_tag: u64,
    template_payload: u64,
    captures_ptr: *const Value,
    count: u64,
) -> JitValue {
    let template_val = Value {
        tag: template_tag,
        payload: template_payload,
    };
    let count = count as usize;

    let closure_template = template_val
        .as_closure()
        .expect("JIT bug: MakeClosure template is not a closure")
        .template
        .clone();

    let env: Vec<Value> = if count == 0 {
        vec![]
    } else {
        let slice = unsafe { std::slice::from_raw_parts(captures_ptr, count) };
        slice.to_vec()
    };

    let result = Value::closure(crate::value::Closure {
        template: closure_template,
        env: std::rc::Rc::new(env),
        squelch_mask: 0,
    });
    JitValue::from_value(result)
}

// =============================================================================
// Internal Helpers
// =============================================================================

/// Convert an ExecResult from execute_bytecode_saving_stack to a `JitValue`.
/// Handles SIG_OK, SIG_HALT (both return the value), SIG_YIELD (returns
/// YIELD_SENTINEL), and errors (signal already set, returns JitValue::nil()).
fn exec_result_to_jit_value(vm: &mut crate::vm::VM, bits: SignalBits) -> JitValue {
    if bits.is_ok() || bits == SIG_HALT {
        let (_, val) = vm.fiber.signal.take().unwrap();
        JitValue::from_value(val)
    } else if bits.contains(SIG_ERROR) {
        // SIG_ERROR — signal already set on fiber
        JitValue::nil()
    } else {
        // Any suspending signal (SIG_YIELD, SIG_SWITCH, user-defined) — side-exit
        YIELD_SENTINEL
    }
}

/// Handle a non-self tail call from JIT code.
///
/// If the target closure has JIT code in the cache, calls it directly.
/// Falls back to TAIL_CALL_SENTINEL (interpreter trampoline) only when
/// the target has no JIT code.
#[no_mangle]
pub extern "C" fn elle_jit_tail_call(
    func_tag: u64,
    func_payload: u64,
    args_ptr: *const Value,
    nargs: u32,
    vm: *mut (),
) -> JitValue {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let func = Value {
        tag: func_tag,
        payload: func_payload,
    };

    // Handle native functions
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
            return JitValue::nil();
        }
        let result = vm.resolve_parameter(id, default);
        return JitValue::from_value(result);
    }

    // Handle closures
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.template.arity, nargs as usize) {
            return JitValue::nil();
        }

        // JIT fast path: if the target has JIT code, call it directly
        let bytecode_ptr = closure.template.bytecode.as_ptr();
        if let Some(jit_code) = vm.jit_cache.get(&bytecode_ptr).cloned() {
            let env_ptr = if closure.env.is_empty() {
                std::ptr::null()
            } else {
                closure.env.as_ptr()
            };

            let result = unsafe {
                jit_code.call(
                    env_ptr,
                    args_ptr,
                    nargs,
                    vm as *mut crate::vm::VM as *mut (),
                    func_tag,
                    func_payload,
                )
            };

            // Check for exception — use contains for compound signals
            if vm
                .fiber
                .signal
                .as_ref()
                .is_some_and(|(b, _)| b.contains(SIG_ERROR) || b.contains(SIG_HALT))
            {
                return JitValue::nil();
            }

            // Check for suspending signal from callee (SIG_YIELD, SIG_SWITCH, user-defined)
            if let Some((sig, _)) = vm.fiber.signal {
                if !sig.is_ok() && !sig.contains(SIG_ERROR) && !sig.contains(SIG_HALT) {
                    return YIELD_SENTINEL;
                }
            }

            if result == YIELD_SENTINEL {
                return YIELD_SENTINEL;
            }

            // Propagate result (including TAIL_CALL_SENTINEL) to caller
            return result;
        }

        // Interpreter fallback — build env, return TAIL_CALL_SENTINEL
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { *args_ptr.add(i) })
            .collect();

        let new_env = build_closure_env_for_jit(closure, &args);
        vm.pending_tail_call = Some(crate::vm::core::TailCallInfo {
            bytecode: closure.template.bytecode.clone(),
            constants: closure.template.constants.clone(),
            env: new_env,
            location_map: closure.template.location_map.clone(),
            squelch_mask: closure.squelch_mask,
        });

        return TAIL_CALL_SENTINEL;
    }

    // Callable collections: struct, array, set, string, bytes
    if let Some(result) =
        crate::vm::call::call_collection(&func, args_ptr_to_value_slice(args_ptr, nargs))
    {
        match result {
            Ok(value) => {
                vm.fiber.signal = Some((SIG_OK, value));
                return JitValue::from_value(value);
            }
            Err((kind, msg)) => {
                vm.fiber.signal = Some((SIG_ERROR, error_val(kind, msg)));
                return JitValue::nil();
            }
        }
    }

    vm.fiber.signal = Some((
        SIG_ERROR,
        error_val("type-error", format!("Cannot call {:?}", func)),
    ));
    JitValue::nil()
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
pub(crate) fn build_closure_env_for_jit(
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
            let fixed_slots = closure.template.num_params - 1;

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

            for (i, arg) in args[..provided_fixed].iter().enumerate() {
                push_param(&mut new_env, closure, i, *arg);
            }
            for i in provided_fixed..fixed_slots {
                push_param(&mut new_env, closure, i, Value::NIL);
            }

            let rest_args = if args.len() > provided_fixed {
                &args[provided_fixed..]
            } else {
                &[]
            };
            let collected = args_to_list(rest_args);
            push_param(&mut new_env, closure, fixed_slots, collected);
        }
        crate::value::Arity::Range(_, max) => {
            for (i, arg) in args.iter().enumerate() {
                push_param(&mut new_env, closure, i, *arg);
            }
            for i in args.len()..max {
                push_param(&mut new_env, closure, i, Value::NIL);
            }
        }
    }

    let num_params = match closure.template.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n,
        crate::value::Arity::Range(min, _) => min,
    };
    let num_locally_defined = closure.template.num_locals.saturating_sub(num_params);

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
    use crate::value::fiber::{SIG_DEBUG, SIG_IO, SIG_OK};
    use crate::vm::VM;

    fn make_vm() -> VM {
        VM::new()
    }

    #[test]
    fn test_has_exception() {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _signals = register_primitives(&mut vm, &mut symbols);

        // Initially no exception
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(false));

        // Set an error signal
        vm.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val("division-by-zero", "test"),
        ));

        // Now should return true
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut () as u64);
        assert_eq!(result, JitValue::bool_val(false));
    }

    // -- jit_handle_primitive_signal: composed signal coverage --

    #[test]
    fn sig_ok_returns_value() {
        let mut vm = make_vm();
        let result = jit_handle_primitive_signal(&mut vm, SIG_OK, Value::int(42));
        assert_eq!(result, JitValue::from_value(Value::int(42)));
        assert!(vm.fiber.signal.is_none());
    }

    #[test]
    fn bare_sig_error_stores_signal_returns_nil() {
        let mut vm = make_vm();
        let err = Value::string("boom");
        let result = jit_handle_primitive_signal(&mut vm, SIG_ERROR, err);
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_ERROR);
    }

    #[test]
    fn composed_sig_error_io_stores_signal_returns_nil() {
        let mut vm = make_vm();
        let bits = SIG_ERROR | SIG_IO;
        let result = jit_handle_primitive_signal(&mut vm, bits, Value::string("io-error"));
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.contains(SIG_ERROR));
        assert!(sig.contains(SIG_IO));
    }

    #[test]
    fn bare_sig_yield_stores_signal_returns_yield_sentinel() {
        let mut vm = make_vm();
        let result = jit_handle_primitive_signal(&mut vm, SIG_YIELD, Value::int(1));
        assert_eq!(result, YIELD_SENTINEL);
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_YIELD);
        assert_eq!(val.as_int(), Some(1));
    }

    #[test]
    fn composed_sig_yield_io_stores_signal_returns_yield_sentinel() {
        let mut vm = make_vm();
        let bits = SIG_YIELD | SIG_IO;
        let result = jit_handle_primitive_signal(&mut vm, bits, Value::int(99));
        assert_eq!(result, YIELD_SENTINEL);
        let (sig, val) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, bits);
        assert_eq!(val.as_int(), Some(99));
    }

    #[test]
    fn sig_halt_stores_signal_returns_nil() {
        let mut vm = make_vm();
        let result = jit_handle_primitive_signal(&mut vm, SIG_HALT, Value::int(0));
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_HALT);
    }

    #[test]
    fn sig_debug_treated_as_suspension() {
        let mut vm = make_vm();
        vm.fiber.signal = Some((SIG_DEBUG, Value::NIL));
        let result = jit_handle_primitive_signal(&mut vm, SIG_DEBUG, Value::NIL);
        assert_eq!(result, YIELD_SENTINEL);
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, SIG_DEBUG);
    }

    #[test]
    fn user_defined_signal_treated_as_suspension() {
        let user_bit = SignalBits::new(1 << 16);
        let mut vm = make_vm();
        vm.fiber.signal = Some((user_bit, Value::NIL));
        let result = jit_handle_primitive_signal(&mut vm, user_bit, Value::NIL);
        assert_eq!(result, YIELD_SENTINEL);
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert_eq!(sig, user_bit);
    }

    #[test]
    fn sig_error_terminal_stored_as_error_not_panic() {
        use crate::value::fiber::SIG_TERMINAL;
        let bits = SIG_ERROR | SIG_TERMINAL;
        let mut vm = make_vm();
        let result = jit_handle_primitive_signal(&mut vm, bits, Value::string("terminal"));
        assert_eq!(result, JitValue::nil());
        let (sig, _) = vm.fiber.signal.take().unwrap();
        assert!(sig.contains(SIG_ERROR));
        assert!(sig.contains(SIG_TERMINAL));
    }
}
