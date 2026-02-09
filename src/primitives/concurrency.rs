use crate::primitives::registration::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::{SendValue, ThreadHandle, Value};
use crate::vm::VM;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// Check if a value is safe to send across thread boundaries.
///
/// A value is safe to send if it contains only immutable data:
/// - Primitives (nil, bool, int, float, symbol, keyword, string)
/// - Immutable collections (vector, struct)
/// - Closures (if their captured environment is safe)
///
/// Unsafe values:
/// - Tables (mutable)
/// - Native functions (contain function pointers)
/// - FFI handles
/// - Thread handles
fn is_value_sendable(value: &Value) -> bool {
    match value {
        // Primitives are always safe
        Value::Nil
        | Value::Bool(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::Symbol(_)
        | Value::Keyword(_)
        | Value::String(_) => true,

        // Immutable collections are safe
        Value::Vector(vec) => {
            // Check all elements are sendable
            vec.iter().all(is_value_sendable)
        }
        Value::Struct(s) => {
            // Check all values are sendable
            s.iter().all(|(_, v)| is_value_sendable(v))
        }

        // Cons cells are safe if their contents are
        Value::Cons(cons) => is_value_sendable(&cons.first) && is_value_sendable(&cons.rest),

        // Closures are safe if their captured environment is safe
        Value::Closure(closure) => closure.env.iter().all(is_value_sendable),

        // Unsafe: mutable tables
        Value::Table(_) => false,

        // Unsafe: native functions (contain function pointers)
        Value::NativeFn(_) => false,

        // Unsafe: FFI handles
        Value::LibHandle(_) | Value::CHandle(_) => false,

        // Unsafe: exceptions and conditions (may contain non-sendable data)
        Value::Exception(_) | Value::Condition(_) => false,

        // Unsafe: thread handles
        Value::ThreadHandle(_) => false,
    }
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
pub fn prim_spawn(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!("spawn: expected 1 argument, got {}", args.len()));
    }

    match &args[0] {
        Value::Closure(closure) => {
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

            // Extract and wrap the closure data for thread safety
            let bytecode_data: Vec<u8> = (*closure.bytecode).clone();
            let constants_data: Vec<SendValue> = closure
                .constants
                .iter()
                .map(|v| SendValue::new(v.clone()))
                .collect();
            let env_data: Vec<SendValue> = closure
                .env
                .iter()
                .map(|v| SendValue::new(v.clone()))
                .collect();

            // Create a holder for the result
            // We wrap the result in SendValue to make it Send
            let result_holder: Arc<Mutex<Option<Result<SendValue, String>>>> =
                Arc::new(Mutex::new(None));
            let result_clone = result_holder.clone();

            // Spawn the thread
            let _handle = std::thread::spawn(move || {
                // Create a fresh VM with primitives registered
                let mut vm = VM::new();
                let mut symbols = SymbolTable::new();
                // Register primitives so they're available in the spawned thread
                register_primitives(&mut vm, &mut symbols);

                // Convert SendValue back to Value for the VM's execute_bytecode
                let bytecode_rc = Rc::new(bytecode_data);
                let constants_rc = Rc::new(
                    constants_data
                        .into_iter()
                        .map(|sv| sv.into_value())
                        .collect::<Vec<_>>(),
                );
                let env_rc = Rc::new(
                    env_data
                        .into_iter()
                        .map(|sv| sv.into_value())
                        .collect::<Vec<_>>(),
                );

                let result = vm.execute_bytecode(&bytecode_rc, &constants_rc, Some(&env_rc));

                // Store the result, wrapping it in SendValue
                if let Ok(mut holder) = result_clone.lock() {
                    *holder = Some(result.map(SendValue::new));
                }
            });

            // Return a thread handle with the result holder
            let thread_handle = ThreadHandle {
                result: result_holder,
            };

            Ok(Value::ThreadHandle(thread_handle))
        }
        Value::NativeFn(_) => {
            Err("spawn: native functions cannot be spawned. Use closures instead.".to_string())
        }
        _ => Err("spawn: argument must be a closure".to_string()),
    }
}

/// Waits for a thread to complete and returns its result
/// (join thread-handle)
///
/// Blocks until the spawned thread completes and returns the actual Value result.
/// If the thread produced an error, that error is re-raised.
pub fn prim_join(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!("join: expected 1 argument, got {}", args.len()));
    }

    match &args[0] {
        Value::ThreadHandle(handle) => {
            // Wait for the result to be available
            // We need to poll since we can't block indefinitely in a primitive
            let mut attempts = 0;
            const MAX_ATTEMPTS: usize = 10000; // ~10 seconds with 1ms sleep

            loop {
                if let Ok(holder) = handle.result.lock() {
                    if let Some(result) = holder.as_ref() {
                        // Result is ready - unwrap SendValue and return
                        return result
                            .as_ref()
                            .map(|send_val| send_val.clone().into_value())
                            .map_err(|e| e.clone());
                    }
                }

                attempts += 1;
                if attempts >= MAX_ATTEMPTS {
                    return Err("join: thread did not complete in time".to_string());
                }

                // Sleep briefly to avoid busy-waiting
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        _ => Err("join: argument must be a thread handle".to_string()),
    }
}

/// Sleeps for the specified number of seconds
/// (sleep seconds)
pub fn prim_sleep(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err(format!("sleep: expected 1 argument, got {}", args.len()));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n < 0 {
                return Err("sleep: duration must be non-negative".to_string());
            }
            std::thread::sleep(std::time::Duration::from_secs(*n as u64));
            Ok(Value::Nil)
        }
        Value::Float(f) => {
            if *f < 0.0 {
                return Err("sleep: duration must be non-negative".to_string());
            }
            std::thread::sleep(std::time::Duration::from_secs_f64(*f));
            Ok(Value::Nil)
        }
        _ => Err("sleep: argument must be a number".to_string()),
    }
}

/// Returns the ID of the current thread
/// (current-thread-id)
pub fn prim_current_thread_id(_args: &[Value]) -> Result<Value, String> {
    let thread_id = std::thread::current().id();
    Ok(Value::String(format!("{:?}", thread_id).into()))
}
