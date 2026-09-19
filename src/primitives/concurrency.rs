// audited: 2026-09-19
//! Thread primitives: `sys/spawn`, `sys/spawn-vm`, `sys/thread-state`,
//! `sys/thread-id`, and `sys/unique`.
//!
//! docs/threads.md
//!
//! The worker a spawn stands up — its stack sizing, its fresh VM, and its
//! completion channel — lives in `worker.rs`.

use crate::primitives::chan::receiver_value;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;
use std::sync::Arc;

mod worker;

/// Shared dispatch for the two spawn primitives. `load_stdlib` picks the
/// worker environment (see `worker::spawn_closure_impl`).
///
/// The closure must:
/// 1. Capture only immutable values (no @structs, native functions, or FFI handles)
/// 2. Take no arguments
/// 3. Return a value
///
/// The spawned thread gets a fresh VM. The closure's bytecode is executed in it.
fn spawn_dispatch(args: &[Value], load_stdlib: bool, ctx: &mut NativeCtx) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        match worker::spawn_closure_impl(closure, load_stdlib, ctx) {
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
