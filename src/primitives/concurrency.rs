use crate::error::{LError, LResult};
use crate::primitives::chan::{receiver_value, SendableValue, WakeList};
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::primitives::registration::register_primitives;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{SendBundle, Value};
use crate::vm::VM;
use std::sync::{Arc, Mutex};

/// Never give a worker less than this — below Rust's own default invites
/// overflow in the runtime itself.
const WORKER_STACK_FLOOR: usize = 2 * 1024 * 1024;
/// Used when the main thread's stack limit is unreadable or unbounded.
const WORKER_STACK_FALLBACK: usize = 8 * 1024 * 1024;
/// Don't reserve a pathologically large stack per worker (e.g. when the main
/// thread's limit is enormous). 64 MiB dwarfs any real compile depth.
const WORKER_STACK_CAP: usize = 64 * 1024 * 1024;

/// Pure policy for the worker stack size, factored out for testing.
///
/// A worker must compile and run anything the main thread can — the test runner
/// ships a file's *syntax* to a worker, which compiles it with its own stdlib,
/// and the frontend's HIR passes (notably `functionalize`) recurse depth-first.
/// Rust's `std::thread::spawn` defaults to a 2 MB stack, far less than the main
/// thread's `RLIMIT_STACK` (commonly 8 MB), so a deep file overflows the worker
/// mid-compile. Resolution order, clamped to `[FLOOR, CAP]`:
///   1. `RUST_MIN_STACK` if set (the same override `std` honors), else
///   2. the main thread's stack limit, else
///   3. a fixed fallback (unbounded/unreadable limit).
fn resolve_worker_stack(env_min_stack: Option<&str>, main_stack: Option<u64>) -> usize {
    if let Some(n) = env_min_stack.and_then(|s| s.trim().parse::<usize>().ok()) {
        return n.clamp(WORKER_STACK_FLOOR, WORKER_STACK_CAP);
    }
    match main_stack {
        // A value too large for usize is pathological — treat as the cap.
        Some(n) => usize::try_from(n)
            .unwrap_or(WORKER_STACK_CAP)
            .clamp(WORKER_STACK_FLOOR, WORKER_STACK_CAP),
        None => WORKER_STACK_FALLBACK,
    }
}

/// The main thread's stack soft limit (`RLIMIT_STACK`), or `None` if it is
/// unreadable, unset, or unbounded (`RLIM_INFINITY`) — cases the caller maps to
/// a fixed fallback rather than trying to reserve an unbounded stack.
fn main_thread_stack_limit() -> Option<u64> {
    // SAFETY: `getrlimit` reads a kernel limit into an out-param we own.
    let mut rl = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_STACK, &mut rl) } != 0 {
        return None;
    }
    if rl.rlim_cur == libc::RLIM_INFINITY || rl.rlim_cur == 0 {
        return None;
    }
    // `rlim_t` is u64 on Linux but its width is platform-dependent, so the
    // widening to u64 is not universally a no-op even when clippy sees it as one
    // on this target.
    #[allow(clippy::useless_conversion)]
    Some(u64::from(rl.rlim_cur))
}

/// Stack size for an `os/spawn` worker thread: see [`resolve_worker_stack`].
fn worker_stack_size() -> usize {
    let env = std::env::var("RUST_MIN_STACK").ok();
    resolve_worker_stack(env.as_deref(), main_thread_stack_limit())
}

/// Helper function to spawn a closure in a new thread.
/// Serializes the closure (validates sendability recursively) and executes it
/// in a fresh VM on a new thread.
///
/// `load_stdlib` selects the worker environment: `false` is the light
/// `sys/spawn-vm` worker (primitives + intrinsics only); `true` is the heavy
/// `sys/spawn` worker, which additionally runs `init_stdlib` so that runtime
/// reflection (`eval`/`read`) in the worker resolves the standard-library
/// vocabulary. See docs/threads.md § Two worker environments.
fn spawn_closure_impl(
    closure: &crate::value::Closure,
    load_stdlib: bool,
    ctx: &mut NativeCtx,
) -> LResult<Value> {
    use crate::value::heap::{HeapObject, ThreadHandle};

    // Serialize the closure (validates sendability recursively). The temporary
    // closure Value is born in the caller's region; `from_value` deep-copies it
    // and the temporary is dropped. The spawning instance's memo names the
    // symbols in the bundle so the worker can print them.
    let closure_val = ctx.closure(closure.clone());
    let sender_symbols = ctx.vm().symbols_ptr;
    let bundle = SendBundle::from_value(closure_val, ctx.heap_mut(), unsafe {
        sender_symbols.as_ref()
    })
    .map_err(|e| LError::generic(format!("spawn: {}", e)))?;

    let result_holder: Arc<Mutex<Option<Result<SendBundle, String>>>> = Arc::new(Mutex::new(None));
    let result_clone = result_holder.clone();

    // Completion channel: the worker signals here AFTER storing its result,
    // so a joiner parked in `chan/select` over `done_rx` wakes exactly once
    // and finds the result already present (race-free) — a scheduler-cooperative
    // wait. The sentinel is an immediate integer (no heap) — trivially safe to
    // cross threads.
    let (done_tx, done_rx) = crossbeam_channel::unbounded::<SendableValue>();
    // The completion channel's wake list carries the spawning instance's trace
    // cell, so a `chan_trace` on the joiner's wake path gates on that instance.
    let done_wake = WakeList::new(ctx.heap_mut().trace_cell());
    let worker_wake = Arc::clone(&done_wake);
    // The worker inherits the spawning VM's Unicode generation: a program is
    // one set of string semantics, whichever VM computes a length.
    let unicode_generation = ctx.unicode_generation();
    // Capabilities flow down across a thread, as they do across a fiber
    // (docs/signals/capabilities.md § Transitivity). The worker runs in a fresh
    // VM whose root fiber is what the capability gate reads, so without this the
    // spawned closure runs with an empty withheld set — and a sandboxed fiber
    // escapes every denial by spawning a thread.
    let withheld = ctx.vm().fiber.withheld;

    // Size the worker's stack to the main thread's (see `worker_stack_size`):
    // the worker compiles arbitrary Elle, and the frontend recurses deep — the
    // 2 MB stack `std::thread::spawn` gives by default overflows on large files.
    let _handle = std::thread::Builder::new()
        .name("elle-os-spawn".into())
        .stack_size(worker_stack_size())
        .spawn(move || {
            // Mask all POSIX signals on this worker so the kernel never
            // selects it as a delivery target. Without this, a user
            // `(spawn closure)` thread inherits the main thread's signal
            // mask at spawn time (typically just the absorb-set from
            // `init_process_signals`) — TERM/INT/QUIT/HUP are unblocked
            // on it, and the kernel may pick it to run the terminate
            // sigaction handler. The handler still calls `_exit`, which
            // is correct end-state, but masking here keeps delivery on
            // the main thread where the rest of the runtime expects it
            // (REPL ^C, watcher-override semantics). Matches the
            // threadpool/JIT/stdin worker discipline.
            crate::io::sigfd::mask_all_signals_on_this_thread();

            // Run the worker body under catch_unwind so that even a panic
            // (e.g. an `.expect` deep in the VM) still finalizes the completion
            // channel below — otherwise a joiner parked in chan/select would
            // wait forever for a wake that never comes.
            let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // This worker's own region heap, owned here rather than leaked.
                // `VM::new` leaks its heap on purpose — right for a VM whose
                // values must outlive it (macro expansion, test scaffolding),
                // wrong for a thread that ends: the result crosses back as a
                // SendBundle (a deep copy), so nothing on this heap is
                // reachable after the join, and a leaked one makes a program
                // that runs workers in sequence pay for every worker it ever
                // ran (docs/threads.md § "A worker owns its heap and gives it
                // back"). Declared before `vm` so it drops LAST — the same
                // order `RuntimeCore` holds its fields in, so the heap is live
                // while the VM, symbols and compile context drop against it.
                // The `Box` also gives the raw `heap_ptr` a stable address.
                let mut heap = Box::new(crate::value::fiberheap::FiberHeap::new());
                let heap_ptr: *mut crate::value::fiberheap::FiberHeap = &mut *heap;
                let mut vm = VM::new_with_heap(heap_ptr);
                vm.set_unicode_generation(unicode_generation);
                let mut symbols = SymbolTable::new();
                // Register primitives so docs are available in the spawned thread.
                // Primitives are in the bytecode constant pool — no globals remapping needed.
                let _signals = register_primitives(&mut vm, &mut symbols);

                // Point this worker's VM at its own symbol table so runtime
                // reflection (`eval`/`read`/`meta`) in the spawned closure resolves
                // names in THIS worker's own table. The local `symbols` is never
                // moved, so its address is stable for the closure body;
                // `vm.symbols()` reborrows the raw pointer per call.
                vm.set_symbols(&mut symbols as *mut SymbolTable);

                // This worker's per-instance compile context (macro expander,
                // core.lisp env, primitive/stdlib metadata, projections), so a
                // runtime `(eval …)` / `(import …)` inside the spawned closure
                // resolves macros and exports. Boxed for a stable address; the VM
                // points at it. Its macro-expansion VM shares THIS worker's heap,
                // as `RuntimeCore` wires the same pair: one worker is one region
                // store, so a macro-expanded value and a runtime value coexist —
                // and the store goes with the thread instead of leaking a second
                // heap per worker (`CompileCtx::new` would build its VM through
                // `VM::new`, which leaks one).
                let mut compile = Box::new(crate::pipeline::CompileCtx::new_with_heap(heap_ptr));
                compile.set_unicode_generation(unicode_generation);
                vm.set_compile_ctx(&mut *compile as *mut crate::pipeline::CompileCtx);

                // Heavy worker (`sys/spawn`): materialize the standard library so
                // runtime reflection resolves stdlib names. The context guards
                // above are in place, which init_stdlib needs (gensym during load).
                // The light worker (`sys/spawn-vm`) skips this — primitives only.
                if load_stdlib {
                    crate::primitives::module_init::init_stdlib(
                        &mut vm,
                        &mut symbols,
                        &mut compile,
                        &crate::compiler::stdlib_cache::StdlibCache::Process,
                    );
                }

                // Arm the guardfree oracle on THIS worker's thread. `GUARD_ARMED`
                // is thread-local and gates the freed-page `mprotect(PROT_NONE)`
                // (src/value/fiberheap/freelog.rs), so a UAF oracle is only as wide
                // as the set of threads that armed it. The heavy worker armed inside
                // init_stdlib above (after its benign init-time frees); the light
                // worker (`sys/spawn-vm`) skips init_stdlib and would otherwise leave
                // its own store unguarded — so a use-after-free on the light worker's
                // recv_region recycle path could never be observed. Arm here, for both
                // worker kinds, before recv_region is minted, so every store the
                // runtime stands up is covered (re-arming the heavy worker is a no-op).
                crate::value::fiberheap::freelog::arm_guard();

                // Coverage pin: when guardfree is requested, this worker's store
                // must be armed, or its recv_region recycle path's use-after-frees
                // are invisible to the oracle (the light worker `sys/spawn-vm` skips
                // init_stdlib, so without the arm above `GUARD_ARMED` stays false on
                // this thread). Trivially true when guardfree is off. Caught by the
                // body's catch_unwind, so a regression surfaces as a `[:failed ...]`
                // join (RED), not a crash.
                debug_assert!(
                    !crate::config::get().has_trace("guardfree")
                        || crate::value::fiberheap::freelog::guard_armed(),
                    "os/spawn worker did not arm the guardfree oracle for its own \
                     store — recv_region recycle-path use-after-frees would be \
                     invisible to --trace=guardfree on this thread",
                );

                // The spawned thread starts with no active alloc region —
                // into_value's heap reconstructions and the closure's execution
                // both need a routing target. Mint a real runtime region from the
                // thread's heap (recycled on free) for the reconstructed closure
                // and its captures; decref it once we've serialized the result
                // back into the SendBundle (which clones values out of this region
                // into its own representation), so the region's RC=1 from alloc
                // reaches 0 and the region is freed before the thread exits.
                // Using a runtime region (not a `new_static_region()` slot used as
                // a physical id) respects the `RuntimeRegion` newtype's
                // static/runtime split and avoids leaking the static-slot counter.
                let recv_region = vm.heap().new_runtime_region();

                // Reconstruct closure from bundle through a ctx over recv_region
                // on this thread's heap, so the whole message tree (the closure and
                // its captured upvalues) is born in recv_region (region coherence
                // across the thread boundary). The captured-LOCAL cells the env
                // construction below adds are the one exception — each gets its own
                // fresh region (see that loop's comment).
                let closure_val = {
                    let mut recv_ctx =
                        crate::primitives::ctx::Alloc::with_region(recv_region, vm.heap());
                    bundle.into_value(&mut recv_ctx, Some(&mut symbols))
                };
                let closure = closure_val
                    .as_closure()
                    .expect("bug: SendBundle root was not a closure")
                    .clone();

                // Location map is carried on the closure template, not the VM.

                // Build execution environment: captured values + NIL slots for locals.
                // Use num_params directly (not derived from arity.min()) — they differ for
                // AtLeast/Range closures.
                let mut env_values: Vec<Value> = closure.env.to_vec();
                let num_locally_defined = closure
                    .template
                    .num_locals
                    .saturating_sub(closure.template.num_params);
                for i in 0..num_locally_defined {
                    if closure.template.capture_locals_mask.is_set(i) {
                        // Each captured-local cell gets its OWN fresh region, never
                        // recv_region. The spawned closure's body owns these cells
                        // and frees them with `DecrefCellRegion` at scope exit —
                        // which is value-resolved to the cell's actual region — just
                        // as a normal (non-spawned) call pairs `MakeCapture` with
                        // `DecrefCellRegion`. Minting them in recv_region instead
                        // would aim that `DecrefCellRegion` at recv_region, driving
                        // its RC to 0 mid-body so the worker's cleanup
                        // `decref_region(recv_region)` double-frees a phantom region.
                        // Pinned by tests/elle/region-spawn-capture-mutate.lisp
                        // (RED before this under the inlined-intrinsic store path).
                        let cell_region = vm.heap().new_runtime_region();
                        env_values.push(crate::value::build::capture_cell(
                            vm.heap(),
                            Value::NIL,
                            cell_region,
                        ));
                    } else {
                        env_values.push(Value::NIL);
                    }
                }

                let env_rc = std::rc::Rc::new(env_values);
                // Bytecode execution allocates into explicit regions (allocating
                // opcodes resolve their static region slots). The body being run
                // IS a closure's, so hand it its executing-closure register via
                // the one-shot and enter through `execute_code` with the
                // template's own `Code` (sharing its bytecode `Rc`, which the
                // dispatch-entry invariant compares by identity) — a
                // self-recursive spawned closure resolves its self-reference to
                // the reconstructed value.
                vm.pending_entry_closure = closure_val;
                // Install the inherited denial for the spawned closure only. The
                // runtime setup above — primitive registration, `init_stdlib`,
                // reconstructing the bundle — is the VM standing itself up, not
                // the sandboxed body, and a worker whose `:error` is withheld
                // could not stand up at all. The worker has no parent to suspend
                // into, so a denial here ends the thread and `sys/join` reports
                // it rather than offering the mediation a fiber gets.
                vm.fiber.withheld = withheld;
                let result = vm.execute_code(closure.template.code(), Some(&env_rc));

                let send_result = match result {
                    Ok(val) => SendBundle::from_value(val, vm.heap(), Some(&symbols))
                        .map_err(|e| format!("Failed to serialize result: {}", e)),
                    Err(e) => Err(e.to_string()),
                };

                // SendBundle::from_value cloned the result out of recv_region into
                // its own representation; the env, closure value, and captures are
                // no longer reachable. Decref recv_region to free everything in it
                // before the thread exits.
                vm.heap().decref_region(recv_region);

                if let Ok(mut holder) = result_clone.lock() {
                    *holder = Some(send_result);
                }
            }));

            // If the body panicked before storing a result, record a failure so
            // a joiner observes `[:failed ...]` rather than hanging.
            if let Ok(mut holder) = result_clone.lock() {
                if holder.is_none() {
                    // Surface the panic payload (an `.expect`/`panic!` message deep
                    // in the VM) rather than a generic string — uninformative
                    // "worker thread panicked" hid which primitive faulted.
                    let detail = match &unwind {
                        Err(p) => p
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .or_else(|| p.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "non-string panic payload".to_string()),
                        Ok(_) => "result never stored".to_string(),
                    };
                    *holder = Some(Err(format!("worker thread panicked: {}", detail)));
                }
            }

            // Signal completion AFTER the result is stored. The sentinel lets a
            // joiner's chan/try-select pick up a ready value; wake_all() pokes the
            // joiner's parked poll fd (cross-thread eventfd write). A joiner that
            // wakes on this signal is guaranteed to find the result present.
            let _ = done_tx.try_send(SendableValue::new(Value::int(1)));
            worker_wake.wake_all();
        })
        .map_err(|e| LError::generic(format!("spawn: failed to start worker thread: {}", e)))?;

    Ok(ctx.alloc(HeapObject::ThreadHandle {
        handle: ThreadHandle::new(result_holder, done_rx, done_wake),
        traits: Value::NIL,
    }))
}

/// Shared dispatch for the two spawn primitives. `load_stdlib` picks the
/// worker environment (see `spawn_closure_impl`).
///
/// The closure must:
/// 1. Capture only immutable values (no @structs, native functions, or FFI handles)
/// 2. Take no arguments
/// 3. Return a value
///
/// The spawned thread gets a fresh VM. The closure's bytecode is executed in it.
fn spawn_dispatch(args: &[Value], load_stdlib: bool, ctx: &mut NativeCtx) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        match spawn_closure_impl(closure, load_stdlib, ctx) {
            Ok(val) => (SIG_OK, val),
            Err(e) => (SIG_ERROR, ctx.error("thread-error", e)),
        }
    } else if args[0].as_native_fn().is_some() {
        (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "spawn: native functions cannot be spawned. Use closures instead.".to_string(),
            ),
        )
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "spawn: argument must be a closure".to_string(),
            ),
        )
    }
}

/// `(sys/spawn closure)` — heavy worker: a fresh VM with primitives AND the
/// standard library loaded, so runtime reflection (`eval`/`read`) in the
/// worker resolves stdlib names. `init_stdlib` runs per spawn — prefer
/// `sys/spawn-vm` when the worker needs only primitives at runtime.
pub(crate) fn prim_spawn(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    spawn_dispatch(args, true, ctx)
}

/// `(sys/spawn-vm closure)` — light worker: a fresh VM with primitives only
/// (plus `%`-intrinsics). The cheap path; eval in the worker resolves
/// primitives/intrinsics but not the standard library.
pub(crate) fn prim_spawn_vm(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    spawn_dispatch(args, false, ctx)
}

/// Non-blocking inspection of a thread handle — a single check, never a
/// loop. The building block for the scheduler-cooperative `sys/join`
/// (defined in stdlib).
/// (sys/thread-state thread-handle)
///
/// Returns one of:
///   - `[:ready value]`  — the thread finished; `value` is its result,
///     reconstructed into the caller's heap from the SendBundle slot.
///   - `[:failed message]` — the thread finished by erroring (or panicked).
///   - `[:pending receiver]` — the thread is still running; `receiver` is a
///     fresh `chan/receiver` over its completion channel, suitable for
///     `chan/select` (which yields to the scheduler rather than polling).
///
/// Peeking the result slot first makes a finished thread (or a repeated
/// join) return immediately without touching the channel — so `sys/join`
/// is idempotent and the common already-done case never yields.
pub(crate) fn prim_thread_state(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(handle) = args[0].as_thread_handle() {
        if let Ok(holder) = handle.result.lock() {
            if let Some(result) = holder.as_ref() {
                return match result {
                    Ok(bundle) => {
                        // Reconstruct into the caller's region (`ctx`).
                        // `ctx` deref-coerces `&mut NativeCtx` → `&mut Alloc`.
                        // The joiner's memo learns the worker's symbol names.
                        let joiner_symbols = ctx.vm().symbols_ptr;
                        let value = bundle
                            .clone()
                            .into_value(ctx, unsafe { joiner_symbols.as_mut() });
                        (SIG_OK, ctx.array(vec![Value::keyword("ready"), value]))
                    }
                    Err(e) => {
                        let msg = ctx.string(e.clone());
                        (SIG_OK, ctx.array(vec![Value::keyword("failed"), msg]))
                    }
                };
            }
        }
        // Still running: hand back a fresh chan/receiver over the completion
        // channel so the caller can chan/select on it (yielding meanwhile).
        let rx = receiver_value(handle.done_rx.clone(), Arc::clone(&handle.done_wake), ctx);
        (SIG_OK, ctx.array(vec![Value::keyword("pending"), rx]))
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "thread-state: argument must be a thread handle".to_string(),
            ),
        )
    }
}

/// The counter behind `sys/unique`. Process-global on purpose: uniqueness must
/// hold across every instance and module in the process, because consumers key
/// process-global tables with it (the scheduler's futex park queues).
static UNIQUE_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// `(sys/unique)` — a fresh integer, unique across the whole process.
///
/// The identity-only alternative to `gensym`: a coordination key (a futex key,
/// a correlation id) needs uniqueness and hashability, not a name, and an
/// integer interns nothing into the symbol table.
/// `tests/elle/sync-keys.lisp` pins that property for the sync constructors.
pub(crate) fn prim_unique(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let n = UNIQUE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    (SIG_OK, Value::int(n as i64))
}

/// Returns the ID of the current thread
/// (current-thread-id)
pub(crate) fn prim_current_thread_id(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
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

// Declarative primitive definitions for concurrency operations
primitive! {
    "sys/spawn" => prim_spawn {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Spawn a thread running a deep-copied closure in a fresh VM with the standard library loaded (so eval/read in the worker resolve stdlib). Heavier than sys/spawn-vm (init_stdlib per spawn).",
        params: &["closure"],
        category: "sys",
        example: "(sys/spawn (fn [] (+ 1 2)))",
        aliases: &["os/spawn"],
        effect: RegionEffect::Fresh,
    }
    "sys/spawn-vm" => prim_spawn_vm {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Spawn a thread running a deep-copied closure in a fresh VM with primitives only (no stdlib). The cheap path; eval in the worker resolves primitives/intrinsics but not stdlib.",
        params: &["closure"],
        category: "sys",
        example: "(sys/spawn-vm (fn [] (+ 1 2)))",
        aliases: &["os/spawn-vm"],
        effect: RegionEffect::Fresh,
    }
    "sys/thread-state" => prim_thread_state {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Inspect a thread handle without blocking (a single check, never a loop): [:ready value], [:failed message], or [:pending receiver]. Building block for sys/join (which adds the scheduler-cooperative wait + timeout).",
        params: &["thread-handle"],
        category: "sys",
        example: "(sys/thread-state thread-handle)",
        aliases: &["os/thread-state"],
        effect: RegionEffect::Fresh,
    }
    "sys/thread-id" => prim_current_thread_id {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return the ID of the current thread",
        category: "sys",
        example: "(sys/thread-id)",
        aliases: &["current-thread-id", "os/thread-id"],
        effect: RegionEffect::Immediate,
    }
    "sys/unique" => prim_unique {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a fresh integer, unique across the whole process. A coordination key (futex key, correlation id) that, unlike a gensym, interns nothing.",
        category: "sys",
        example: "(sys/unique)",
        aliases: &[],
        effect: RegionEffect::Immediate,
    }
}

#[cfg(test)]
mod tests;
