use crate::primitives::registration::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};
use crate::vm::VM;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Check if a value is safe to send across thread boundaries.
///
/// A value is safe to send if it contains only immutable data:
/// - Primitives (nil, bool, int, float, symbol, keyword, string)
/// - Immutable collections (array, struct)
/// - Closures (if their captured environment is safe)
///
/// Unsafe values:
/// - Tables (mutable)
/// - Native functions (contain function pointers)
/// - FFI handles
/// - Thread handles
fn is_value_sendable(value: &Value) -> bool {
    use crate::value::heap::{deref, HeapObject};

    // Check immediate values
    if value.is_nil()
        || value.is_empty_list()
        || value.is_bool()
        || value.is_int()
        || value.is_float()
        || value.is_symbol()
        || value.is_keyword()
        || value.is_string()
    {
        return true;
    }

    // Check heap values
    if !value.is_heap() {
        return false;
    }

    match unsafe { deref(*value) } {
        // Strings are immutable and safe
        HeapObject::String(_) => true,

        // Immutable collections are safe
        HeapObject::Array(vec) => {
            if let Ok(borrowed) = vec.try_borrow() {
                borrowed.iter().all(is_value_sendable)
            } else {
                false
            }
        }
        HeapObject::Struct(s) => s.iter().all(|(_, v)| is_value_sendable(v)),

        // Tuples are safe if their contents are
        HeapObject::Tuple(elems) => elems.iter().all(is_value_sendable),

        // Cons cells are safe if their contents are
        HeapObject::Cons(cons) => is_value_sendable(&cons.first) && is_value_sendable(&cons.rest),

        // Closures are safe if their captured environment is safe
        // Note: Closure uses new Value type
        HeapObject::Closure(closure) => closure.env.iter().all(is_value_sendable),

        // Unsafe: mutable tables
        HeapObject::Table(_) => false,

        // Unsafe: native functions (contain function pointers)
        HeapObject::NativeFn(_) => false,

        // Unsafe: FFI handles
        HeapObject::LibHandle(_) | HeapObject::CHandle(_, _) => false,

        // Unsafe: thread handles
        HeapObject::ThreadHandle(_) => false,

        // Cells are safe if their contents are sendable
        HeapObject::Cell(cell, _) => {
            if let Ok(val) = cell.try_borrow() {
                is_value_sendable(&val)
            } else {
                // If we can't borrow, assume it's not sendable (to be safe)
                false
            }
        }

        // Float values that couldn't be stored inline
        HeapObject::Float(_) => true,

        // Fibers are not sendable (contain execution state with closures)
        HeapObject::Fiber(_) => false,

        // Syntax objects are not sendable (contain Rc)
        HeapObject::Syntax(_) => false,

        // Bindings are compile-time only, not sendable
        HeapObject::Binding(_) => false,
    }
}

/// Helper function to spawn a closure in a new thread
/// Extracts closure data, validates sendability, and executes in a fresh VM
fn spawn_closure_impl(closure: &crate::value::Closure) -> Result<Value, String> {
    use crate::value::SendValue;
    use std::collections::HashMap;

    // Check that all captured values are sendable
    for (i, captured) in closure.env.iter().enumerate() {
        if !is_value_sendable(captured) {
            return Err(format!(
                "spawn: closure captures mutable or unsafe value at position {} ({})",
                i,
                captured.type_name()
            ));
        }
    }

    // Also check constants for sendability
    for (i, constant) in closure.constants.iter().enumerate() {
        if !is_value_sendable(constant) {
            return Err(format!(
                "spawn: closure has non-sendable constant at position {} ({})",
                i,
                constant.type_name()
            ));
        }
    }

    // Deep-copy environment and constants using SendValue
    let env_send: Result<Vec<SendValue>, String> = closure
        .env
        .iter()
        .map(|v| SendValue::from_value(*v))
        .collect();
    let env_send = env_send.map_err(|e| format!("spawn: failed to copy environment: {}", e))?;

    let constants_send: Result<Vec<SendValue>, String> = closure
        .constants
        .iter()
        .map(|v| SendValue::from_value(*v))
        .collect();
    let constants_send =
        constants_send.map_err(|e| format!("spawn: failed to copy constants: {}", e))?;

    // Extract the closure bytecode for thread safety
    let bytecode_data: Vec<u8> = (*closure.bytecode).clone();

    // Extract closure metadata needed for proper environment setup
    let num_locals = closure.num_locals;
    let _num_captures = closure.num_captures;
    let arity = match closure.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n,
        crate::value::Arity::Range(min, _) => min,
    };

    // Extract symbol names for cross-thread portability
    // This allows remapping symbol IDs in the new thread's symbol table
    let symbol_names_for_thread: HashMap<u32, String> = (*closure.symbol_names).clone();

    // Extract location map for error reporting in the spawned thread
    let location_map_for_thread: std::collections::HashMap<usize, crate::error::SourceLoc> =
        (*closure.location_map).clone();

    // Create a holder for the result
    let result_holder: Arc<Mutex<Option<Result<crate::value::SendValue, String>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result_holder.clone();

    // Spawn the thread
    let _handle = std::thread::spawn(move || {
        // Create a fresh VM with primitives registered
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        // Register primitives so they're available in the spawned thread
        let _effects = register_primitives(&mut vm, &mut symbols);

        // Remap globals so bytecode symbol IDs resolve correctly.
        // The bytecode was compiled with symbol IDs from the parent thread's symbol table.
        // The new thread has a fresh symbol table with potentially different IDs.
        // We need to ensure that when the bytecode looks up a symbol by its old ID,
        // it finds the correct value (which was registered under a new ID).
        for (old_id, name) in &symbol_names_for_thread {
            // Find what register_primitives registered this name under
            if let Some(new_id) = symbols.get(name) {
                if new_id.0 != *old_id {
                    // The bytecode expects this symbol under old_id, but register_primitives
                    // put it under new_id. Copy the value to the old_id slot.
                    if let Some(val) = vm
                        .globals
                        .get(new_id.0 as usize)
                        .filter(|v| !v.is_undefined())
                        .copied()
                    {
                        let idx = *old_id as usize;
                        if idx >= vm.globals.len() {
                            vm.globals.resize(idx + 1, Value::UNDEFINED);
                        }
                        vm.globals[idx] = val;
                    }
                }
            }
        }

        // Reconstruct values from SendValue
        let bytecode_rc = Rc::new(bytecode_data);
        let mut env_values: Vec<Value> = env_send
            .into_iter()
            .map(|sv: SendValue| sv.into_value())
            .collect();
        let constants_values: Vec<Value> = constants_send
            .into_iter()
            .map(|sv: SendValue| sv.into_value())
            .collect();

        // Add LocalCell slots for locally-defined variables (let bindings etc.)
        // This replicates the logic in the VM's Call handler that creates
        // environment slots for variables defined inside the closure body.
        let num_params = arity;
        let num_locally_defined = num_locals.saturating_sub(num_params);
        for _ in 0..num_locally_defined {
            env_values.push(Value::local_cell(Value::NIL));
        }

        let env_rc = Rc::new(env_values);
        let constants_rc = Rc::new(constants_values);

        // Set the location map for error reporting in the spawned thread
        vm.set_location_map(location_map_for_thread);

        let result = vm.execute_bytecode(&bytecode_rc, &constants_rc, Some(&env_rc));

        let send_result = match result {
            Ok(val) => {
                SendValue::from_value(val).map_err(|e| format!("Failed to serialize result: {}", e))
            }
            Err(e) => Err(e.to_string()),
        };

        // Store the result
        if let Ok(mut holder) = result_clone.lock() {
            *holder = Some(send_result);
        }
    });

    // Return a thread handle with the result holder
    use crate::value::heap::{alloc, HeapObject, ThreadHandleData};
    let thread_handle_data = ThreadHandleData {
        result: result_holder,
    };
    Ok(alloc(HeapObject::ThreadHandle(thread_handle_data)))
}

/// Spawns a new thread that executes a closure with captured immutable values
/// (spawn closure)
///
/// The closure must:
/// 1. Capture only immutable values (no tables, native functions, or FFI handles)
/// 2. Take no arguments
/// 3. Return a value
///
/// The spawned thread gets a fresh VM with only primitives registered.
/// The closure's bytecode is compiled and executed in that VM.
///
/// For JIT-compiled closures, falls back to the source closure for thread-safe execution.
pub fn prim_spawn(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("spawn: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(closure) = args[0].as_closure() {
        match spawn_closure_impl(closure) {
            Ok(val) => (SIG_OK, val),
            Err(e) => (SIG_ERROR, error_val("error", e)),
        }
    } else if args[0].as_native_fn().is_some() {
        (
            SIG_ERROR,
            error_val(
                "error",
                "spawn: native functions cannot be spawned. Use closures instead.".to_string(),
            ),
        )
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "spawn: argument must be a closure".to_string(),
            ),
        )
    }
}

/// Waits for a thread to complete and returns its result
/// (join thread-handle)
///
/// Blocks until the spawned thread completes and returns the actual Value result.
/// If the thread produced an error, that error is re-raised.
pub fn prim_join(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("join: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    if let Some(handle) = args[0].as_thread_handle() {
        // Wait for the result to be available
        // We need to poll since we can't block indefinitely in a primitive
        let mut attempts = 0;
        const MAX_ATTEMPTS: usize = 10000; // ~10 seconds with 1ms sleep

        loop {
            if let Ok(holder) = handle.result.lock() {
                if let Some(result) = holder.as_ref() {
                    // Result is ready - convert from SendValue back to Value
                    return match result {
                        Ok(send_val) => (SIG_OK, send_val.clone().into_value()),
                        Err(e) => (SIG_ERROR, error_val("error", e.clone())),
                    };
                }
            }

            attempts += 1;
            if attempts >= MAX_ATTEMPTS {
                return (
                    SIG_ERROR,
                    error_val("error", "join: thread did not complete in time".to_string()),
                );
            }

            // Sleep briefly to avoid busy-waiting
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                "join: argument must be a thread handle".to_string(),
            ),
        )
    }
}

/// Returns the ID of the current thread
/// (current-thread-id)
pub fn prim_current_thread_id(_args: &[Value]) -> (SignalBits, Value) {
    let thread_id = std::thread::current().id();
    (SIG_OK, Value::string(format!("{:?}", thread_id)))
}
