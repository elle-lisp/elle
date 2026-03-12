use crate::effects::Effect;
use crate::error::{LError, LResult};
use crate::primitives::def::PrimitiveDef;
use crate::primitives::registration::register_primitives;
use crate::symbol::SymbolTable;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, SendBundle, Value};
use crate::vm::VM;
use std::sync::{Arc, Mutex};

/// Helper function to spawn a closure in a new thread.
/// Serializes the closure (validates sendability recursively) and executes it
/// in a fresh VM on a new thread.
fn spawn_closure_impl(closure: &crate::value::Closure) -> LResult<Value> {
    use crate::value::heap::{alloc, HeapObject, ThreadHandle};

    // Serialize the closure (validates sendability recursively).
    let bundle = SendBundle::from_value(Value::closure(closure.clone()))
        .map_err(|e| LError::generic(format!("spawn: {}", e)))?;

    let result_holder: Arc<Mutex<Option<Result<SendBundle, String>>>> = Arc::new(Mutex::new(None));
    let result_clone = result_holder.clone();

    let _handle = std::thread::spawn(move || {
        let mut vm = VM::new();
        let mut symbols = SymbolTable::new();
        // Register primitives so docs are available in the spawned thread.
        // Primitives are in the bytecode constant pool — no globals remapping needed.
        let _effects = register_primitives(&mut vm, &mut symbols);

        // Reconstruct closure from bundle.
        let closure_val = bundle.into_value();
        let closure = closure_val
            .as_closure()
            .expect("bug: SendBundle root was not a closure")
            .clone();

        // Set location map for error reporting in the spawned thread.
        vm.set_location_map((*closure.template.location_map).clone());

        // Build execution environment: captured values + NIL slots for locals.
        // Use num_params directly (not derived from arity.min()) — they differ for
        // AtLeast/Range closures. The old code had this wrong.
        let mut env_values: Vec<Value> = closure.env.to_vec();
        let num_locally_defined = closure
            .template
            .num_locals
            .saturating_sub(closure.template.num_params);
        for i in 0..num_locally_defined {
            if i >= 64 || (closure.template.lbox_locals_mask & (1 << i)) != 0 {
                env_values.push(Value::local_lbox(Value::NIL));
            } else {
                env_values.push(Value::NIL);
            }
        }

        let env_rc = std::rc::Rc::new(env_values);
        let result = vm.execute_bytecode(
            &closure.template.bytecode,
            &closure.template.constants,
            Some(&env_rc),
        );

        let send_result = match result {
            Ok(val) => SendBundle::from_value(val)
                .map_err(|e| format!("Failed to serialize result: {}", e)),
            Err(e) => Err(e.to_string()),
        };

        if let Ok(mut holder) = result_clone.lock() {
            *holder = Some(send_result);
        }
    });

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
                        Ok(bundle) => (SIG_OK, bundle.clone().into_value()),
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
