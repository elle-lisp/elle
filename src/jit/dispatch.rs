//! Runtime dispatch helpers for JIT-compiled code
//!
//! These functions handle complex operations that interact with heap types
//! or require VM access: data structures, cells, globals, and function calls.

use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_HALT, SIG_OK, SIG_QUERY};
use crate::value::repr::TAG_NIL;
use crate::value::{error_val, Value};

// =============================================================================
// Primitive Signal Handling (for JIT dispatch)
// =============================================================================

/// Handle signal bits from a primitive call in JIT context.
///
/// JIT-compiled code only runs non-suspending functions, so SIG_YIELD and
/// SIG_RESUME should never appear here. SIG_ERROR sets the exception on
/// the fiber for the JIT caller to check. SIG_QUERY is dispatched to the
/// VM's query handler (for primitives like `list-primitives` and
/// `primitive-meta` that read VM state).
fn jit_handle_primitive_signal(vm: &mut crate::vm::VM, bits: SignalBits, value: Value) -> u64 {
    match bits {
        SIG_OK => value.to_bits(),
        SIG_ERROR | SIG_HALT => {
            vm.fiber.signal = Some((bits, value));
            TAG_NIL
        }
        SIG_QUERY => {
            // Dispatch VM state query and return the result.
            let (sig, result) = vm.dispatch_query(value);
            if sig == SIG_ERROR {
                vm.fiber.signal = Some((SIG_ERROR, result));
                TAG_NIL
            } else {
                result.to_bits()
            }
        }
        _ => {
            // Reaching here means the effect system has a bug: a suspending
            // primitive was called from JIT-compiled code, which should be
            // impossible since the JIT gate rejects may_suspend() closures.
            panic!(
                "Effect system bug: signal {} reached JIT-compiled code. \
                 Only SIG_OK, SIG_ERROR, SIG_HALT, and SIG_QUERY should appear in JIT context.",
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

// =============================================================================
// Exception Checking
// =============================================================================

/// Check if a terminal signal is pending on the VM (error or halt).
/// Returns TRUE bits if one is set, FALSE bits otherwise.
#[no_mangle]
pub extern "C" fn elle_jit_has_exception(vm: *mut ()) -> u64 {
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
// Global Variable Access
// =============================================================================

/// Load a global variable by symbol ID
#[no_mangle]
pub extern "C" fn elle_jit_load_global(sym_id: u64, vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let sym = sym_id as u32;
    match vm.globals.get(sym as usize).filter(|v| !v.is_undefined()) {
        Some(val) => val.to_bits(),
        None => {
            vm.fiber.signal = Some((
                SIG_ERROR,
                error_val("error", format!("Undefined global: {}", sym)),
            ));
            TAG_NIL
        }
    }
}

/// Store a global variable by symbol ID
#[no_mangle]
pub extern "C" fn elle_jit_store_global(sym_id: u64, value: u64, vm: *mut ()) -> u64 {
    let vm = unsafe { &mut *(vm as *mut crate::vm::VM) };
    let sym = sym_id as u32;
    let val = unsafe { Value::from_bits(value) };
    let idx = sym as usize;
    if idx >= vm.globals.len() {
        vm.globals.resize(idx + 1, Value::UNDEFINED);
    }
    vm.globals[idx] = val;
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

    // Dispatch to closure
    if let Some(closure) = func.as_closure() {
        // Check arity using nargs directly — no Vec needed
        if !vm.check_arity(&closure.arity, nargs as usize) {
            return TAG_NIL;
        }

        // JIT-to-JIT fast path: check if callee has JIT code
        let bytecode_ptr = closure.bytecode.as_ptr();
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

            // Handle tail call sentinel
            if result_bits == TAIL_CALL_SENTINEL {
                if let Some((tail_bc, tail_consts, tail_env)) = vm.pending_tail_call.take() {
                    match vm.execute_closure_bytecode(&tail_bc, &tail_consts, &tail_env) {
                        Ok(val) => return val.to_bits(),
                        Err(e) => {
                            vm.fiber.signal = Some((SIG_ERROR, error_val("error", e)));
                            return TAG_NIL;
                        }
                    }
                }
            }

            return result_bits;
        }

        // Interpreter fallback — reconstruct args Vec for env building
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { Value::from_bits(*args_ptr.add(i)) })
            .collect();

        let new_env = build_closure_env_for_jit(closure, &args);

        vm.fiber.call_depth += 1;
        let result = vm.execute_closure_bytecode(&closure.bytecode, &closure.constants, &new_env);
        vm.fiber.call_depth -= 1;

        match result {
            Ok(val) => val.to_bits(),
            Err(e) => {
                vm.fiber.signal = Some((SIG_ERROR, error_val("error", e)));
                TAG_NIL
            }
        }
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
    if let Some((tail_bc, tail_consts, tail_env)) = vm.pending_tail_call.take() {
        match vm.execute_closure_bytecode(&tail_bc, &tail_consts, &tail_env) {
            Ok(val) => val.to_bits(),
            Err(e) => {
                vm.fiber.signal = Some((SIG_ERROR, error_val("error", e)));
                TAG_NIL
            }
        }
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

    // Handle closures
    if let Some(closure) = func.as_closure() {
        if !vm.check_arity(&closure.arity, nargs as usize) {
            return TAG_NIL;
        }

        // JIT fast path: if the target has JIT code, call it directly
        let bytecode_ptr = closure.bytecode.as_ptr();
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

            // Propagate result (including TAIL_CALL_SENTINEL) to caller
            return result_bits;
        }

        // Interpreter fallback — build env, return TAIL_CALL_SENTINEL
        let args: Vec<Value> = (0..nargs as usize)
            .map(|i| unsafe { Value::from_bits(*args_ptr.add(i)) })
            .collect();

        let new_env = build_closure_env_for_jit(closure, &args);
        vm.pending_tail_call = Some((closure.bytecode.clone(), closure.constants.clone(), new_env));

        return TAIL_CALL_SENTINEL;
    }

    vm.fiber.signal = Some((
        SIG_ERROR,
        error_val("type-error", format!("Cannot call {:?}", func)),
    ));
    TAG_NIL
}

/// Build a closure environment from captured variables and arguments.
/// This is a copy of VM::build_closure_env but standalone for JIT use.
fn build_closure_env_for_jit(
    closure: &crate::value::Closure,
    args: &[Value],
) -> std::rc::Rc<Vec<Value>> {
    let mut new_env = Vec::with_capacity(closure.env_capacity());
    new_env.extend((*closure.env).iter().cloned());

    // Add parameters, wrapping in local cells if cell_params_mask indicates
    for (i, arg) in args.iter().enumerate() {
        if i < 64 && (closure.cell_params_mask & (1 << i)) != 0 {
            new_env.push(Value::local_cell(*arg));
        } else {
            new_env.push(*arg);
        }
    }

    // Calculate number of locally-defined variables
    let num_params = match closure.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n,
        crate::value::Arity::Range(min, _) => min,
    };
    let num_locally_defined = closure.num_locals.saturating_sub(num_params);

    // Add empty LocalCells for locally-defined variables
    for _ in 0..num_locally_defined {
        new_env.push(Value::local_cell(Value::NIL));
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
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut ());
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(false));

        // Set an error signal
        vm.fiber.signal = Some((
            crate::value::SIG_ERROR,
            crate::value::error_val("division-by-zero", "test"),
        ));

        // Now should return true
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut ());
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(true));

        // Clear signal
        vm.fiber.signal = None;

        // Should return false again
        let result = elle_jit_has_exception(&mut vm as *mut VM as *mut ());
        let val = unsafe { Value::from_bits(result) };
        assert_eq!(val.as_bool(), Some(false));
    }
}
