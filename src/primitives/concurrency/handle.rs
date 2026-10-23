// audited: 2026-10-22
//! What a thread handle answers: `sys/thread-state`, the state a join reads, and `sys/kill`.
//!
//! docs/threads.md

use crate::primitives::chan::receiver_value;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::WorkerOutcome;
use crate::value::types::Arity;
use crate::value::Value;
use std::sync::Arc;

/// Non-blocking inspection of a thread handle — a single check, never a
/// loop. The building block for the scheduler-cooperative `sys/join`
/// (defined in stdlib).
/// (sys/thread-state thread-handle)
///
/// Returns one of:
///   - `[:ready value]`  — the thread finished; `value` is its result,
///     reconstructed into the caller's heap from the SendBundle slot.
///   - `[:failed message]` — the thread finished by erroring (or panicked).
///   - `[:killed]` — the thread was killed (docs/threads.md).
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
            if let Some(outcome) = holder.as_ref() {
                return match outcome {
                    WorkerOutcome::Ready(bundle) => {
                        // Reconstruct into the caller's region (`ctx`).
                        // `ctx` deref-coerces `&mut NativeCtx` → `&mut Alloc`.
                        // The joiner's memo learns the worker's symbol names.
                        let joiner_symbols = ctx.vm().symbols_ptr;
                        let value = bundle
                            .clone()
                            .into_value(ctx, unsafe { joiner_symbols.as_mut() });
                        (SIG_OK, ctx.array(vec![Value::keyword("ready"), value]))
                    }
                    WorkerOutcome::Failed(e) => {
                        let msg = ctx.string(e.clone());
                        (SIG_OK, ctx.array(vec![Value::keyword("failed"), msg]))
                    }
                    WorkerOutcome::Killed => (SIG_OK, ctx.array(vec![Value::keyword("killed")])),
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

/// `(sys/kill thread-handle)` — pull the worker's kill switch, which ends it
/// at its next charge site with the terminal `:killed` error
/// (docs/threads.md). A kill after the worker's end, or a second kill,
/// changes nothing. Answers nil.
pub(crate) fn prim_kill(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(handle) = args[0].as_thread_handle() {
        handle.kill.pull();
        (SIG_OK, Value::NIL)
    } else {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "kill: argument must be a thread handle".to_string(),
            ),
        )
    }
}

primitive! {
    "sys/thread-state" => prim_thread_state {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Inspect a thread handle without blocking (a single check, never a loop): [:ready value], [:failed message], [:killed], or [:pending receiver]. Building block for sys/join (which adds the scheduler-cooperative wait + timeout).",
        params: &["thread-handle"],
        category: "sys",
        example: "(sys/thread-state thread-handle)",
        aliases: &["os/thread-state"],
        effect: RegionEffect::Fresh,
    }
    "sys/kill" => prim_kill {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Kill the worker behind a thread handle: at its next charge site, each call and each backward jump, it raises the terminal error {:error :killed}, which no mask catches, and a join on it raises the same. A kill after the worker's end, or a second kill, changes nothing. Answers nil.",
        params: &["thread-handle"],
        category: "sys",
        example: "(sys/kill thread-handle)",
        aliases: &["os/kill"],
        effect: RegionEffect::Immediate,
    }
}
