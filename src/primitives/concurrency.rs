use crate::effects::Effect;
use crate::error::{LError, LResult};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::registration::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
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
        HeapObject::LString(_) => true,

        // Immutable collections are safe
        HeapObject::LArrayMut(vec) => {
            if let Ok(borrowed) = vec.try_borrow() {
                borrowed.iter().all(is_value_sendable)
            } else {
                false
            }
        }
        HeapObject::LStruct(s) => s.iter().all(|(_, v)| is_value_sendable(v)),

        // Arrays (immutable) are safe if their contents are
        HeapObject::LArray(elems) => elems.iter().all(is_value_sendable),

        // Cons cells are safe if their contents are
        HeapObject::Cons(cons) => is_value_sendable(&cons.first) && is_value_sendable(&cons.rest),

        // Closures are safe if their captured environment is safe
        // Note: Closure uses new Value type
        HeapObject::Closure(closure) => {
            closure.env.iter().all(is_value_sendable)
                && closure.template.constants.iter().all(is_value_sendable)
        }

        // Unsafe: mutable @structs
        HeapObject::LStructMut(_) => false,

        // Native function pointers are inherently Send + Sync
        HeapObject::NativeFn(_) => true,

        // Unsafe: FFI handles
        HeapObject::LibHandle(_) => false,

        // Unsafe: thread handles
        HeapObject::ThreadHandle(_) => false,

        // Boxes are safe if their contents are sendable
        HeapObject::LBox(cell, _) => {
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

        // FFI signatures are not sendable
        HeapObject::FFISignature(_, _) => false,

        // FFI type descriptors are pure data — safe to send
        HeapObject::FFIType(_) => true,

        // @string values are sendable if we deep-copy
        HeapObject::LStringMut(buf) => buf.try_borrow().is_ok(),

        // Bytes are immutable and sendable
        HeapObject::LBytes(_) => true,

        // @bytes values are sendable if we deep-copy
        HeapObject::LBytesMut(blob) => blob.try_borrow().is_ok(),

        // Managed pointers are not sendable (Cell is not thread-safe)
        HeapObject::ManagedPointer(_) => false,

        // External objects are not sendable (contain Rc<dyn Any>)
        HeapObject::External(_) => false,

        // Parameters are not sendable (fiber-local state)
        HeapObject::Parameter { .. } => false,

        // Sets (immutable) are safe if their contents are
        HeapObject::LSet(s) => s.iter().all(is_value_sendable),

        // Sets (mutable) are sendable if we deep-copy
        HeapObject::LSetMut(s_ref) => {
            if let Ok(s) = s_ref.try_borrow() {
                s.iter().all(is_value_sendable)
            } else {
                false
            }
        }
    }
}

/// Helper function to spawn a closure in a new thread
/// Extracts closure data, validates sendability, and executes in a fresh VM
fn spawn_closure_impl(closure: &crate::value::Closure) -> LResult<Value> {
    use crate::value::SendValue;

    // Check that all captured values are sendable
    for (i, captured) in closure.env.iter().enumerate() {
        if !is_value_sendable(captured) {
            return Err(LError::generic(format!(
                "spawn: closure captures mutable or unsafe value at position {} ({})",
                i,
                captured.type_name()
            )));
        }
    }

    // Also check constants for sendability
    for (i, constant) in closure.template.constants.iter().enumerate() {
        if !is_value_sendable(constant) {
            return Err(LError::generic(format!(
                "spawn: closure has non-sendable constant at position {} ({})",
                i,
                constant.type_name()
            )));
        }
    }

    // Deep-copy environment and constants using SendValue
    let env_send: Result<Vec<SendValue>, String> = closure
        .env
        .iter()
        .map(|v| SendValue::from_value(*v))
        .collect();
    let env_send = env_send
        .map_err(|e| LError::generic(format!("spawn: failed to copy environment: {}", e)))?;

    let constants_send: Result<Vec<SendValue>, String> = closure
        .template
        .constants
        .iter()
        .map(|v| SendValue::from_value(*v))
        .collect();
    let constants_send = constants_send
        .map_err(|e| LError::generic(format!("spawn: failed to copy constants: {}", e)))?;

    // Extract the closure bytecode for thread safety
    let bytecode_data: Vec<u8> = (*closure.template.bytecode).clone();

    // Extract closure metadata needed for proper environment setup
    let num_locals = closure.template.num_locals;
    let lbox_locals_mask = closure.template.lbox_locals_mask;
    let _num_captures = closure.template.num_captures;
    let arity = match closure.template.arity {
        crate::value::Arity::Exact(n) => n,
        crate::value::Arity::AtLeast(n) => n,
        crate::value::Arity::Range(min, _) => min,
    };

    // Extract location map for error reporting in the spawned thread
    let location_map_for_thread: std::collections::HashMap<usize, crate::error::SourceLoc> =
        (*closure.template.location_map).clone();

    // Create a holder for the result
    let result_holder: Arc<Mutex<Option<Result<crate::value::SendBundle, String>>>> =
        Arc::new(Mutex::new(None));
    let result_clone = result_holder.clone();

    // Spawn the thread
    let _handle = std::thread::spawn(move || {
        // Create a fresh VM with primitives registered
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        // Register primitives so docs are available in the spawned thread.
        // Primitives are in the bytecode constant pool — no globals remapping needed.
        let _effects = register_primitives(&mut vm, &mut symbols);

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

        // Add slots for locally-defined variables.
        // cell-wrapped locals get LocalCell(NIL); non-cell locals get bare NIL.
        // Beyond index 63, conservatively use LocalCell.
        let num_params = arity;
        let num_locally_defined = num_locals.saturating_sub(num_params);
        for i in 0..num_locally_defined {
            if i >= 64 || (lbox_locals_mask & (1 << i)) != 0 {
                env_values.push(Value::local_lbox(Value::NIL));
            } else {
                env_values.push(Value::NIL);
            }
        }

        let env_rc = Rc::new(env_values);
        let constants_rc = Rc::new(constants_values);

        // Set the location map for error reporting in the spawned thread
        vm.set_location_map(location_map_for_thread);

        let result = vm.execute_bytecode(&bytecode_rc, &constants_rc, Some(&env_rc));

        let send_result: Result<crate::value::SendBundle, String> = match result {
            Ok(val) => SendValue::from_value(val)
                .map(|sv| crate::value::SendBundle {
                    root: sv,
                    closures: vec![],
                })
                .map_err(|e| format!("Failed to serialize result: {}", e)),
            Err(e) => Err(e.to_string()),
        };

        // Store the result
        if let Ok(mut holder) = result_clone.lock() {
            *holder = Some(send_result);
        }
    });

    // Return a thread handle with the result holder
    use crate::value::heap::{alloc, HeapObject, ThreadHandle};
    let thread_handle_data = ThreadHandle {
        result: result_holder,
    };
    Ok(alloc(HeapObject::ThreadHandle(thread_handle_data)))
}

/// Spawns a new thread that executes a closure with captured immutable values
/// (spawn closure)
///
/// The closure must:
/// 1. Capture only immutable values (no @structs, native functions, or FFI handles)
/// 2. Take no arguments
/// 3. Return a value
///
/// The spawned thread gets a fresh VM with only primitives registered.
/// The closure's bytecode is compiled and executed in that VM.
///
/// For JIT-compiled closures, falls back to the source closure for thread-safe execution.
pub(crate) fn prim_spawn(args: &[Value]) -> (SignalBits, Value) {
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
/// If the thread produced an error, that error is propagated.
pub(crate) fn prim_join(args: &[Value]) -> (SignalBits, Value) {
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
                        Ok(bundle) => (SIG_OK, bundle.clone().root.into_value()),
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
pub(crate) fn prim_current_thread_id(_args: &[Value]) -> (SignalBits, Value) {
    let id = std::thread::current().id();
    // ThreadId debug format is "ThreadId(N)" — extract the integer
    let s = format!("{:?}", id);
    let n: i64 = s
        .trim_start_matches("ThreadId(")
        .trim_end_matches(')')
        .parse()
        .unwrap_or(0);
    (SIG_OK, Value::int(n))
}

/// Declarative primitive definitions for concurrency operations
pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sys/spawn",
        func: prim_spawn,
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Spawn a new thread that executes a closure with captured immutable values",
        params: &["closure"],
        category: "sys",
        example: "(sys/spawn (fn [] (+ 1 2)))",
        aliases: &["spawn", "os/spawn"],
    },
    PrimitiveDef {
        name: "sys/join",
        func: prim_join,
        effect: Effect::errors(),
        arity: Arity::Exact(1),
        doc: "Wait for a thread to complete and return its result",
        params: &["thread-handle"],
        category: "sys",
        example: "(sys/join thread-handle)",
        aliases: &["join", "os/join"],
    },
    PrimitiveDef {
        name: "sys/thread-id",
        func: prim_current_thread_id,
        effect: Effect::inert(),
        arity: Arity::Exact(0),
        doc: "Return the ID of the current thread",
        params: &[],
        category: "sys",
        example: "(sys/thread-id)",
        aliases: &["current-thread-id", "os/thread-id"],
    },
];
