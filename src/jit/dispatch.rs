//! Runtime dispatch helpers for JIT-compiled code
//!
//! These functions handle complex operations that interact with heap types
//! or require VM access: data structures, cells, globals, and function calls.

use crate::value::fiber::{
    SignalBits, SIG_CANCEL, SIG_ERROR, SIG_HALT, SIG_OK, SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
    SIG_YIELD,
};
use crate::value::repr::TAG_NIL;
use crate::value::{error_val, SuspendedFrame, Value};

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
pub struct YieldPointMeta {
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
pub struct CallSiteMeta {
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
// Data Construction
// =============================================================================

/// Allocate a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cons(car: u64, cdr: u64) -> u64 {
    let car = unsafe { Value::from_bits(car) };
    let cdr = unsafe { Value::from_bits(cdr) };
    Value::cons(car, cdr).to_bits()
}

/// Extract car from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_car(pair_bits: u64) -> u64 {
    let pair = unsafe { Value::from_bits(pair_bits) };
    match pair.as_cons() {
        Some(cons) => cons.first.to_bits(),
        None => super::runtime::elle_jit_type_error_str("pair"),
    }
}

/// Extract cdr from a cons cell
#[no_mangle]
pub extern "C" fn elle_jit_cdr(pair_bits: u64) -> u64 {
    let pair = unsafe { Value::from_bits(pair_bits) };
    match pair.as_cons() {
        Some(cons) => cons.rest.to_bits(),
        None => super::runtime::elle_jit_type_error_str("pair"),
    }
}

/// Allocate an array from a list of elements
#[no_mangle]
pub extern "C" fn elle_jit_make_array(elements: *const u64, count: u32) -> u64 {
    let mut vec = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let bits = unsafe { *elements.add(i) };
        vec.push(unsafe { Value::from_bits(bits) });
    }
    Value::array(vec).to_bits()
}

/// Check if value is a pair (cons cell)
#[no_mangle]
pub extern "C" fn elle_jit_is_pair(a: u64) -> u64 {
    let val = unsafe { Value::from_bits(a) };
    Value::bool(val.is_cons()).to_bits()
}

// =============================================================================
// Cell Operations
// =============================================================================

/// Create a LocalCell wrapping a value
#[no_mangle]
pub extern "C" fn elle_jit_make_cell(value: u64) -> u64 {
    let val = unsafe { Value::from_bits(value) };
    Value::local_cell(val).to_bits()
}

/// Load value from a LocalCell
#[no_mangle]
pub extern "C" fn elle_jit_load_cell(cell_bits: u64) -> u64 {
    let cell = unsafe { Value::from_bits(cell_bits) };
    if let Some(cell_ref) = cell.as_cell() {
        cell_ref.borrow().to_bits()
    } else {
        super::runtime::elle_jit_type_error_str("cell")
    }
}

/// Load from env slot, auto-unwrapping LocalCell if present.
/// This matches the interpreter's LoadUpvalue semantics:
/// - LocalCell (compiler-created mutable capture): unwrap and return inner value
/// - Everything else (plain value, user Cell, etc.): return as-is
#[no_mangle]
pub extern "C" fn elle_jit_load_capture(val_bits: u64) -> u64 {
    let val = unsafe { Value::from_bits(val_bits) };
    if val.is_local_cell() {
        if let Some(cell_ref) = val.as_cell() {
            cell_ref.borrow().to_bits()
        } else {
            val_bits // shouldn't happen, but safe fallback
        }
    } else {
        val_bits
    }
}

/// Store value into a LocalCell
#[no_mangle]
pub extern "C" fn elle_jit_store_cell(cell_bits: u64, value: u64) -> u64 {
    let cell = unsafe { Value::from_bits(cell_bits) };
    let val = unsafe { Value::from_bits(value) };
    if let Some(cell_ref) = cell.as_cell() {
        *cell_ref.borrow_mut() = val;
        TAG_NIL
    } else {
        super::runtime::elle_jit_type_error_str("cell")
    }
}

/// Store to a capture slot, handling cells automatically
/// If the slot contains a LocalCell, stores into the cell.
/// Otherwise, stores directly to the slot.
#[no_mangle]
pub extern "C" fn elle_jit_store_capture(env_ptr: *mut u64, index: u64, value: u64) -> u64 {
    let idx = index as usize;
    let slot_bits = unsafe { *env_ptr.add(idx) };
    let slot = unsafe { Value::from_bits(slot_bits) };

    if slot.is_local_cell() {
        // Store into the cell
        if let Some(cell_ref) = slot.as_cell() {
            let new_val = unsafe { Value::from_bits(value) };
            *cell_ref.borrow_mut() = new_val;
        }
    } else {
        // Direct store to the slot
        unsafe {
            *env_ptr.add(idx) = value;
        }
    }
    TAG_NIL
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
// Yield Side-Exit Helpers
// =============================================================================

/// JIT yield side-exit: build a SuspendedFrame and set fiber.signal.
///
/// Called from JIT code when a Yield terminator is reached.
/// All parameters are u64 to match the Cranelift I64 calling convention.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous u64 values
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield(
    yielded_value: u64,
    spilled_values: u64, // *const u64 as u64
    yield_index: u64,
    vm: u64, // *mut () as u64
    closure_bits: u64,
) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let yielded = unsafe { Value::from_bits(yielded_value) };
    let closure_val = unsafe { Value::from_bits(closure_bits) };

    let closure = closure_val
        .as_closure()
        .expect("VM bug: elle_jit_yield called with non-closure self_bits");

    // Look up yield point metadata from JitCode
    let bytecode_ptr = closure.template.bytecode.as_ptr();
    let jit_code = vm
        .jit_cache
        .get(&bytecode_ptr)
        .expect("VM bug: elle_jit_yield called but no JitCode in cache");
    let yield_meta = &jit_code.yield_points[yield_index as usize];
    let num_locals = yield_meta.num_locals as usize;
    let num_operands = yield_meta.num_spilled as usize;
    let total_spilled = num_locals + num_operands;

    // Build the stack from spilled values.
    // The JIT spills in interpreter layout: [locals..., operands...].
    // The SuspendedFrame.stack must match what the interpreter would have
    // captured via `self.fiber.stack.drain(..).collect()`.
    let spilled_ptr = spilled_values as *const u64;
    let mut stack = Vec::with_capacity(total_spilled);
    for i in 0..total_spilled {
        let bits = unsafe { *spilled_ptr.add(i) };
        stack.push(unsafe { Value::from_bits(bits) });
    }

    let frame = SuspendedFrame {
        bytecode: closure.template.bytecode.clone(),
        constants: closure.template.constants.clone(),
        env: closure.env.clone(),
        ip: yield_meta.resume_ip,
        stack,
        active_allocator: crate::value::fiber_heap::save_active_allocator(),
        location_map: closure.template.location_map.clone(),
    };

    vm.fiber.signal = Some((SIG_YIELD, yielded));
    vm.fiber.suspended = Some(vec![frame]);

    YIELD_SENTINEL
}

/// JIT yield-through-call: append a caller frame to fiber.suspended.
///
/// Called from JIT code when a callee yields (detected by post-call
/// signal check). Builds a caller SuspendedFrame and appends it to
/// the existing suspended frame chain.
///
/// All parameters are u64 to match the Cranelift I64 calling convention.
///
/// Looks up call site metadata from `JitCode.call_sites` using
/// `call_site_index`, analogous to how `elle_jit_yield` uses
/// `YieldPointMeta`.
///
/// # Safety
/// `spilled_values` must point to `num_spilled` contiguous u64 values
/// (or be null when num_spilled is 0).
#[no_mangle]
pub extern "C" fn elle_jit_yield_through_call(
    spilled_values: u64, // *const u64 as u64
    call_site_index: u64,
    vm: u64, // *mut () as u64
    closure_bits: u64,
) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let closure_val = unsafe { Value::from_bits(closure_bits) };

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

    let spilled_ptr = spilled_values as *const u64;
    let n = call_meta.num_spilled as usize;
    let mut stack = Vec::with_capacity(n);
    for i in 0..n {
        let bits = unsafe { *spilled_ptr.add(i) };
        stack.push(unsafe { Value::from_bits(bits) });
    }

    let caller_frame = SuspendedFrame {
        bytecode: closure.template.bytecode.clone(),
        constants: closure.template.constants.clone(),
        env: closure.env.clone(),
        ip: call_meta.resume_ip,
        stack,
        active_allocator: crate::value::fiber_heap::save_active_allocator(),
        location_map: closure.template.location_map.clone(),
    };

    // Append caller frame to the existing suspended chain.
    // The callee MUST have set fiber.suspended — if not, it's a VM bug.
    let frames = vm.fiber.suspended.as_mut().expect(
        "VM bug: elle_jit_yield_through_call called but fiber.suspended is None. \
         The callee should have set fiber.suspended before returning YIELD_SENTINEL.",
    );
    frames.push(caller_frame);

    YIELD_SENTINEL
}

/// Check if any signal (error, halt, or yield) is pending on the VM.
/// Returns TRUE bits if set, FALSE bits otherwise.
///
/// This extends `elle_jit_has_exception` to also detect SIG_YIELD.
/// Used after Call instructions in yielding functions.
#[no_mangle]
pub extern "C" fn elle_jit_has_signal(vm: u64) -> u64 {
    let vm = unsafe { &*(vm as *const crate::vm::VM) };
    Value::bool(matches!(
        vm.fiber.signal,
        Some((SIG_ERROR | SIG_HALT | SIG_YIELD, _))
    ))
    .to_bits()
}

// =============================================================================
// Environment Building
// =============================================================================

/// Push a parameter value into the environment buffer, wrapping in a
/// LocalCell if the cell_params_mask indicates it's needed.
#[inline]
fn push_param(buf: &mut Vec<Value>, closure: &crate::value::Closure, i: usize, val: Value) {
    if i < 64 && (closure.template.cell_params_mask & (1 << i)) != 0 {
        buf.push(Value::local_cell(val));
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
    // Cell-wrapped locals get LocalCell(NIL); non-cell locals get bare NIL.
    // Beyond index 63, conservatively use LocalCell.
    for i in 0..num_locally_defined {
        if i >= 64 || (closure.template.cell_locals_mask & (1 << i)) != 0 {
            new_env.push(Value::local_cell(Value::NIL));
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
    fn test_cons_car_cdr() {
        let car = Value::int(1).to_bits();
        let cdr = Value::int(2).to_bits();
        let pair = elle_jit_cons(car, cdr);

        let car_result = elle_jit_car(pair);
        let cdr_result = elle_jit_cdr(pair);

        let car_val = unsafe { Value::from_bits(car_result) };
        let cdr_val = unsafe { Value::from_bits(cdr_result) };

        assert_eq!(car_val.as_int(), Some(1));
        assert_eq!(cdr_val.as_int(), Some(2));
    }

    #[test]
    fn test_is_pair() {
        let pair = elle_jit_cons(Value::int(1).to_bits(), Value::int(2).to_bits());
        let is_pair = unsafe { Value::from_bits(elle_jit_is_pair(pair)) };
        assert_eq!(is_pair.as_bool(), Some(true));

        let not_pair = unsafe { Value::from_bits(elle_jit_is_pair(Value::int(42).to_bits())) };
        assert_eq!(not_pair.as_bool(), Some(false));
    }

    #[test]
    fn test_make_array() {
        let elements = [
            Value::int(1).to_bits(),
            Value::int(2).to_bits(),
            Value::int(3).to_bits(),
        ];
        let vec_bits = elle_jit_make_array(elements.as_ptr(), 3);
        let vec = unsafe { Value::from_bits(vec_bits) };

        assert!(vec.is_array());
        let vec_ref = vec.as_array().unwrap();
        let borrowed = vec_ref.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0].as_int(), Some(1));
        assert_eq!(borrowed[1].as_int(), Some(2));
        assert_eq!(borrowed[2].as_int(), Some(3));
    }

    #[test]
    fn test_cell_operations() {
        // Make a cell
        let cell_bits = elle_jit_make_cell(Value::int(42).to_bits());
        let cell = unsafe { Value::from_bits(cell_bits) };
        assert!(cell.is_local_cell());

        // Load from cell
        let loaded = elle_jit_load_cell(cell_bits);
        let loaded_val = unsafe { Value::from_bits(loaded) };
        assert_eq!(loaded_val.as_int(), Some(42));

        // Store to cell
        elle_jit_store_cell(cell_bits, Value::int(100).to_bits());

        // Load again
        let loaded2 = elle_jit_load_cell(cell_bits);
        let loaded_val2 = unsafe { Value::from_bits(loaded2) };
        assert_eq!(loaded_val2.as_int(), Some(100));
    }

    #[test]
    fn test_has_exception() {
        use crate::primitives::register_primitives;
        use crate::symbol::SymbolTable;
        use crate::vm::VM;

        let mut symbols = SymbolTable::new();
        let mut vm = VM::new();
        let _effects = register_primitives(&mut vm, &mut symbols);

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

    // =========================================================================
    // JIT yield: SuspendedFrame layout invariant
    //
    // The JIT spills registers in interpreter stack order:
    //   [param_0, ..., param_{n-1}, local_0, ..., local_m, operand_0, ..., operand_k]
    //
    // elle_jit_yield reads this buffer and builds a SuspendedFrame whose
    // `stack` field must match what the interpreter's handle_yield would
    // produce by draining its operand stack.
    //
    // These tests verify that coupling by calling elle_jit_yield with a
    // known spilled buffer and checking the resulting SuspendedFrame.
    // =========================================================================

    /// Set up a VM + Closure + JitCode for yield tests.
    /// Returns (vm, closure_value) with the JitCode already in jit_cache.
    fn setup_yield_test(
        bytecode: Vec<u8>,
        constants: Vec<Value>,
        env: Vec<Value>,
        yield_points: Vec<YieldPointMeta>,
    ) -> (crate::vm::VM, Value) {
        use crate::effects::Effect;
        use crate::value::types::Arity;
        use crate::value::ClosureTemplate;
        use std::collections::HashMap;
        use std::rc::Rc;

        let bytecode = Rc::new(bytecode);
        let constants = Rc::new(constants);
        let env = Rc::new(env);

        let template = Rc::new(ClosureTemplate {
            bytecode: bytecode.clone(),
            arity: Arity::Exact(0),
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants,
            effect: Effect::yields(),
            cell_params_mask: 0,
            cell_locals_mask: 0,
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
            jit_code: None,
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
        });

        let closure = crate::value::Closure {
            template: template.clone(),
            env,
        };

        // bytecode_ptr must be captured before Value::closure moves the Closure
        let bytecode_ptr = template.bytecode.as_ptr();
        let closure_val = Value::closure(closure);

        let jit_code = Rc::new(crate::jit::JitCode::test_with_yield_points(yield_points));

        let mut vm = crate::vm::VM::new();
        vm.jit_cache.insert(bytecode_ptr, jit_code);

        (vm, closure_val)
    }

    #[test]
    fn test_jit_yield_builds_correct_suspended_frame() {
        // 2 params, 1 local, 3 operands
        let yield_meta = YieldPointMeta {
            resume_ip: 42,
            num_spilled: 3, // operand count
            num_locals: 3,  // params + locally-defined = 2 + 1
        };

        let bytecode = vec![0xAA; 10]; // dummy bytecode
        let constants = vec![Value::int(999)];
        let env = vec![Value::int(777)];

        let (mut vm, closure_val) = setup_yield_test(
            bytecode.clone(),
            constants.clone(),
            env.clone(),
            vec![yield_meta],
        );

        // Spilled buffer: [param0, param1, local0, op0, op1, op2]
        let spilled: Vec<u64> = vec![
            Value::int(10).to_bits(), // param 0
            Value::int(20).to_bits(), // param 1
            Value::int(30).to_bits(), // local 0
            Value::int(40).to_bits(), // operand 0
            Value::int(50).to_bits(), // operand 1
            Value::int(60).to_bits(), // operand 2
        ];

        let yielded = Value::int(100);

        let result = elle_jit_yield(
            yielded.to_bits(),
            spilled.as_ptr() as u64,
            0, // yield_index
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        assert_eq!(result, YIELD_SENTINEL);

        // Check signal
        let (sig, val) = vm.fiber.signal.unwrap();
        assert_eq!(sig, SIG_YIELD);
        assert_eq!(val.as_int(), Some(100));

        // Check suspended frame
        let frames = vm.fiber.suspended.as_ref().unwrap();
        assert_eq!(frames.len(), 1);
        let frame = &frames[0];

        assert_eq!(frame.ip, 42);
        assert_eq!(&*frame.bytecode, &bytecode);
        assert_eq!(&*frame.constants, &constants);
        assert_eq!(&*frame.env, &env);

        // Stack must contain all spilled values in order:
        // [param0, param1, local0, op0, op1, op2]
        assert_eq!(frame.stack.len(), 6);
        assert_eq!(frame.stack[0].as_int(), Some(10)); // param 0
        assert_eq!(frame.stack[1].as_int(), Some(20)); // param 1
        assert_eq!(frame.stack[2].as_int(), Some(30)); // local 0
        assert_eq!(frame.stack[3].as_int(), Some(40)); // operand 0
        assert_eq!(frame.stack[4].as_int(), Some(50)); // operand 1
        assert_eq!(frame.stack[5].as_int(), Some(60)); // operand 2
    }

    #[test]
    fn test_jit_yield_zero_locals_zero_operands() {
        // Edge case: nothing to spill
        let yield_meta = YieldPointMeta {
            resume_ip: 0,
            num_spilled: 0,
            num_locals: 0,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<u64> = vec![];
        let yielded = Value::NIL;

        let result = elle_jit_yield(
            yielded.to_bits(),
            spilled.as_ptr() as u64,
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        assert_eq!(result, YIELD_SENTINEL);

        let frames = vm.fiber.suspended.as_ref().unwrap();
        let frame = &frames[0];
        assert_eq!(frame.stack.len(), 0);
        assert_eq!(frame.ip, 0);
    }

    #[test]
    fn test_jit_yield_only_operands_no_locals() {
        // 0 locals, 2 operands
        let yield_meta = YieldPointMeta {
            resume_ip: 10,
            num_spilled: 2,
            num_locals: 0,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0x01], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<u64> = vec![Value::int(1).to_bits(), Value::int(2).to_bits()];

        elle_jit_yield(
            Value::int(0).to_bits(),
            spilled.as_ptr() as u64,
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        let frame = &vm.fiber.suspended.as_ref().unwrap()[0];
        assert_eq!(frame.stack.len(), 2);
        assert_eq!(frame.stack[0].as_int(), Some(1));
        assert_eq!(frame.stack[1].as_int(), Some(2));
    }

    #[test]
    fn test_jit_yield_only_locals_no_operands() {
        // 3 locals (params + locally-defined), 0 operands
        let yield_meta = YieldPointMeta {
            resume_ip: 5,
            num_spilled: 0,
            num_locals: 3,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0x02], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<u64> = vec![
            Value::int(100).to_bits(),
            Value::int(200).to_bits(),
            Value::int(300).to_bits(),
        ];

        elle_jit_yield(
            Value::int(0).to_bits(),
            spilled.as_ptr() as u64,
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        let frame = &vm.fiber.suspended.as_ref().unwrap()[0];
        assert_eq!(frame.stack.len(), 3);
        assert_eq!(frame.stack[0].as_int(), Some(100));
        assert_eq!(frame.stack[1].as_int(), Some(200));
        assert_eq!(frame.stack[2].as_int(), Some(300));
    }

    #[test]
    fn test_jit_yield_large_spill() {
        // Stress test: 10 locals, 20 operands
        let yield_meta = YieldPointMeta {
            resume_ip: 99,
            num_spilled: 20,
            num_locals: 10,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0xFF], vec![], vec![], vec![yield_meta]);

        let mut spilled: Vec<u64> = Vec::with_capacity(30);
        for i in 0..30 {
            spilled.push(Value::int(i).to_bits());
        }

        elle_jit_yield(
            Value::int(0).to_bits(),
            spilled.as_ptr() as u64,
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        let frame = &vm.fiber.suspended.as_ref().unwrap()[0];
        assert_eq!(frame.stack.len(), 30);
        for i in 0..30 {
            assert_eq!(
                frame.stack[i].as_int(),
                Some(i as i64),
                "stack[{}] mismatch",
                i
            );
        }
        assert_eq!(frame.ip, 99);
    }

    #[test]
    fn test_jit_yield_multiple_yield_points() {
        // Two yield points with different metadata
        let yield_points = vec![
            YieldPointMeta {
                resume_ip: 10,
                num_spilled: 1,
                num_locals: 2,
            },
            YieldPointMeta {
                resume_ip: 20,
                num_spilled: 3,
                num_locals: 1,
            },
        ];

        let (mut vm, closure_val) =
            setup_yield_test(vec![0x01, 0x02], vec![], vec![], yield_points);

        // Test yield point 1 (index 1): 1 local + 3 operands = 4 values
        let spilled: Vec<u64> = vec![
            Value::int(10).to_bits(), // local 0
            Value::int(20).to_bits(), // operand 0
            Value::int(30).to_bits(), // operand 1
            Value::int(40).to_bits(), // operand 2
        ];

        elle_jit_yield(
            Value::int(0).to_bits(),
            spilled.as_ptr() as u64,
            1, // yield_index = 1
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        let frame = &vm.fiber.suspended.as_ref().unwrap()[0];
        assert_eq!(frame.ip, 20); // resume_ip from yield point 1
        assert_eq!(frame.stack.len(), 4);
    }

    #[test]
    fn test_jit_yield_preserves_value_types() {
        // Verify non-integer value types survive the spill/restore cycle
        let yield_meta = YieldPointMeta {
            resume_ip: 0,
            num_spilled: 2,
            num_locals: 2,
        };

        let (mut vm, closure_val) = setup_yield_test(vec![0x01], vec![], vec![], vec![yield_meta]);

        let spilled: Vec<u64> = vec![
            Value::NIL.to_bits(),        // local: nil
            Value::bool(true).to_bits(), // local: bool
            Value::float(1.5).to_bits(), // operand: float
            Value::EMPTY_LIST.to_bits(), // operand: empty list
        ];

        elle_jit_yield(
            Value::int(0).to_bits(),
            spilled.as_ptr() as u64,
            0,
            &mut vm as *mut crate::vm::VM as *mut () as u64,
            closure_val.to_bits(),
        );

        let frame = &vm.fiber.suspended.as_ref().unwrap()[0];
        assert_eq!(frame.stack.len(), 4);
        assert!(frame.stack[0].is_nil());
        assert_eq!(frame.stack[1].as_bool(), Some(true));
        assert_eq!(frame.stack[2].as_float(), Some(1.5));
        assert!(frame.stack[3].is_empty_list());
    }
}
