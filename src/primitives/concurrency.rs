// audited: 2026-10-22
//! Thread primitives: `sys/spawn`, `sys/spawn-vm`, `sys/thread-id`, and
//! `sys/unique`.
//!
//! docs/threads.md
//!
//! The worker a spawn stands up — its stack sizing, its fresh VM, its kill
//! switch and its completion channel — lives in `worker.rs`, and what its
//! handle answers, `sys/thread-state` and `sys/kill`, in `handle.rs`.

use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

pub(crate) mod handle;
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
/// worker resolves stdlib names. The worker boots as the spawning instance
/// did: from its boot image, or by compiling (docs/impl/image/boot.md).
pub(crate) fn prim_spawn(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    spawn_dispatch(args, true, ctx)
}

/// `(sys/spawn-vm closure)` — light worker: a fresh VM with primitives only
/// (plus `%`-intrinsics). Eval in the worker resolves primitives and
/// intrinsics but not the standard library.
pub(crate) fn prim_spawn_vm(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    spawn_dispatch(args, false, ctx)
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
/// `tests/impl/sync-keys.lisp` pins that property for the sync constructors.
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
        doc: "Spawn a thread running a deep-copied closure in a fresh VM with the standard library loaded (so eval/read in the worker resolve stdlib). The worker hydrates the spawning instance's boot image, or compiles the stdlib when that instance has none.",
        params: &["closure"],
        category: "sys",
        example: "(sys/spawn (fn [] (+ 1 2)))",
        aliases: &["os/spawn"],
        effect: RegionEffect::Fresh,
    }
    "sys/spawn-vm" => prim_spawn_vm {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Spawn a thread running a deep-copied closure in a fresh VM with primitives only (no stdlib). Eval in the worker resolves primitives and intrinsics but not stdlib.",
        params: &["closure"],
        category: "sys",
        example: "(sys/spawn-vm (fn [] (+ 1 2)))",
        aliases: &["os/spawn-vm"],
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
