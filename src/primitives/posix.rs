//! POSIX signal primitives — send, raise, watch, next, close, plus
//! introspection (pending / mask / watching).
//!
//! See docs/posix-signals.md for the user-facing contract. Naming is
//! `os/sig-*` to disambiguate from elle's runtime "signal" concept.

use crate::io::request::{IoOp, IoRequest};
use crate::io::sigfd::SignalReceiver;
use crate::io::sigmap;
use crate::primitives::ctx::NativeCtx;
use crate::primitives::def::RegionEffect;
use crate::signals::{Signal, SIG_OS_SIGNAL};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::Value;
use std::collections::BTreeSet;

/// Resolve a Value (single keyword, set/array/list of keywords) to a list
/// of unique libc signums. Unknown names or non-keywords are an error.
fn resolve_signal_set(
    val: &Value,
    context: &str,
    ctx: &mut NativeCtx,
) -> Result<Vec<libc::c_int>, (SignalBits, Value)> {
    fn push(out: &mut Vec<libc::c_int>, signum: libc::c_int) {
        if !out.contains(&signum) {
            out.push(signum);
        }
    }

    let mut out: Vec<libc::c_int> = Vec::new();

    // All `ctx.error` calls below are `&self`, so every closure captures `ctx`
    // by shared reference — they coexist without aliasing conflicts.
    let process_keyword =
        |name: &str, out: &mut Vec<libc::c_int>| -> Result<(), (SignalBits, Value)> {
            match sigmap::keyword_to_signum(name) {
                Some(s) => {
                    push(out, s);
                    Ok(())
                }
                None => Err((
                    SIG_ERROR,
                    ctx.error(
                        "argument-error",
                        format!(
                            "{}: unknown signal keyword :{}; expected one of {}",
                            context,
                            name,
                            sigmap::supported_list_str()
                        ),
                    ),
                )),
            }
        };

    // Single keyword.
    if let Some(name) = val.as_keyword_name() {
        process_keyword(&name, &mut out)?;
        return Ok(out);
    }

    // Set of keywords.
    if let Some(set) = val.as_set() {
        for elem in set.iter() {
            let name = elem.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "{}: set elements must be keywords, got {}",
                            context,
                            elem.type_name()
                        ),
                    ),
                )
            })?;
            process_keyword(&name, &mut out)?;
        }
        return Ok(out);
    }

    // Array / mutable array of keywords.
    if let Some(elems) = val.as_array() {
        for elem in elems.iter() {
            let name = elem.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "{}: array elements must be keywords, got {}",
                            context,
                            elem.type_name()
                        ),
                    ),
                )
            })?;
            process_keyword(&name, &mut out)?;
        }
        return Ok(out);
    }
    if let Some(arr) = val.as_array_mut() {
        for elem in arr.borrow().iter() {
            let name = elem.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "{}: array elements must be keywords, got {}",
                            context,
                            elem.type_name()
                        ),
                    ),
                )
            })?;
            process_keyword(&name, &mut out)?;
        }
        return Ok(out);
    }

    // List of keywords.
    if val.as_pair().is_some() {
        let mut current = *val;
        while let Some(pair) = current.as_pair() {
            let name = pair.first.as_keyword_name().ok_or_else(|| {
                (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "{}: list elements must be keywords, got {}",
                            context,
                            pair.first.type_name()
                        ),
                    ),
                )
            })?;
            process_keyword(&name, &mut out)?;
            current = pair.rest;
        }
        return Ok(out);
    }

    if val.is_empty_list() {
        return Ok(out);
    }

    Err((
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "{}: expected keyword, set, array, or list of keywords, got {}",
                context,
                val.type_name()
            ),
        ),
    ))
}

/// Build a Value-set from a slice of libc signums.
fn signums_to_keyword_set(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    signums: &[libc::c_int],
) -> Value {
    let mut set: BTreeSet<Value> = BTreeSet::new();
    for &s in signums {
        if let Some(name) = sigmap::signum_to_keyword(s) {
            set.insert(Value::keyword(name));
        }
    }
    ctx.set(set)
}

// ── os/sig-send ────────────────────────────────────────────────────────

fn prim_sig_send(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let pid = match args[0].as_int() {
        Some(n) => n as i32,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "os/sig-send: pid must be integer, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let signum = match sigmap::resolve(&args[1], "os/sig-send") {
        Ok(s) => s,
        Err(e) => {
            let (kind, msg) = e.parts("os/sig-send");
            return (SIG_ERROR, ctx.error(kind, msg));
        }
    };
    let trace = ctx.heap_mut().trace_cell();
    crate::io::sigfd::posix_trace(
        &trace,
        format_args!("prim_sig_send kill(pid={}, signum={})", pid, signum),
    );
    let ret = unsafe { libc::kill(pid, signum) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        crate::io::sigfd::posix_trace(
            &trace,
            format_args!("prim_sig_send kill FAILED errno={}", err),
        );
        return (
            SIG_ERROR,
            ctx.error("os-signal-error", format!("os/sig-send: {}", err)),
        );
    }
    (SIG_OK, Value::NIL)
}

// ── os/sig-raise ───────────────────────────────────────────────────────

fn prim_sig_raise(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let signum = match sigmap::resolve(&args[0], "os/sig-raise") {
        Ok(s) => s,
        Err(e) => {
            let (kind, msg) = e.parts("os/sig-raise");
            return (SIG_ERROR, ctx.error(kind, msg));
        }
    };
    // libc::raise sends to the calling thread; for kqueue/signalfd we want
    // it delivered to the process, so use kill(getpid()) instead. On
    // single-threaded targets they're equivalent.
    let trace = ctx.heap_mut().trace_cell();
    crate::io::sigfd::posix_trace(
        &trace,
        format_args!("prim_sig_raise kill(getpid(), signum={})", signum),
    );
    let ret = unsafe { libc::kill(libc::getpid(), signum) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        crate::io::sigfd::posix_trace(
            &trace,
            format_args!("prim_sig_raise kill FAILED errno={}", err),
        );
        return (
            SIG_ERROR,
            ctx.error("os-signal-error", format!("os/sig-raise: {}", err)),
        );
    }
    (SIG_OK, Value::NIL)
}

// ── os/sig-watch ───────────────────────────────────────────────────────

fn prim_sig_watch(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let signums = match resolve_signal_set(&args[0], "os/sig-watch", ctx) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if signums.is_empty() {
        return (
            SIG_ERROR,
            ctx.error(
                "argument-error",
                "os/sig-watch: signal set must be non-empty",
            ),
        );
    }
    // Hand the receiver this instance's trace cell (captured here at watch time),
    // so every `posix_trace` on its signalfd/kqueue path — including the blocking
    // read that runs on a threadpool worker — gates on this instance.
    let trace = ctx.heap_mut().trace_cell();
    match SignalReceiver::new(signums, trace) {
        Ok(r) => (SIG_OK, ctx.external("signal-receiver", r)),
        Err(msg) => (SIG_ERROR, ctx.error("os-signal-error", msg)),
    }
}

// ── os/sig-next ────────────────────────────────────────────────────────

fn prim_sig_next(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args[0].as_external::<SignalReceiver>().is_none() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                "os/sig-next: argument must be a signal receiver",
            ),
        );
    }
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(ctx, IoOp::SigNext, args[0]),
    )
}

// ── os/sig-close ───────────────────────────────────────────────────────

fn prim_sig_close(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let receiver = match args[0].as_external::<SignalReceiver>() {
        Some(r) => r,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "os/sig-close: argument must be a signal receiver",
                ),
            )
        }
    };
    receiver.close();
    (SIG_OK, Value::NIL)
}

// ── os/sig-pending ─────────────────────────────────────────────────────

fn prim_sig_pending(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let signums = crate::io::sigfd::current_thread_pending();
    (SIG_OK, signums_to_keyword_set(ctx, &signums))
}

// ── os/sig-mask ────────────────────────────────────────────────────────

fn prim_sig_mask(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let signums = crate::io::sigfd::current_thread_blocked();
    (SIG_OK, signums_to_keyword_set(ctx, &signums))
}

// ── os/sig-watching ────────────────────────────────────────────────────

fn prim_sig_watching(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let signums = crate::io::sigfd::currently_watched();
    (SIG_OK, signums_to_keyword_set(ctx, &signums))
}

primitive! {
    "os/sig-send" => prim_sig_send {
        signal: Signal::os_signal_errors(),
        arity: Arity::Exact(2),
        doc: "Send a POSIX signal to a pid. signum is a keyword (:sigterm, :sigkill, etc.) or a named integer. Capability: :os-signal.",
        params: &["pid", "signum"],
        category: "posix",
        example: "(os/sig-send 4242 :sigterm)",
        effect: RegionEffect::Immediate,
    }
    "os/sig-raise" => prim_sig_raise {
        signal: Signal::os_signal_errors(),
        arity: Arity::Exact(1),
        doc: "Send a POSIX signal to the current process. Capability: :os-signal.",
        params: &["signum"],
        category: "posix",
        example: "(os/sig-raise :sigusr1)",
        effect: RegionEffect::Immediate,
    }
    "os/sig-watch" => prim_sig_watch {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Open a signal receiver that watches a set of POSIX signals. Blocks the signals on the calling thread and queues deliveries onto a kernel fd. Returns a SignalReceiver. See docs/posix-signals.md for mask policy.",
        params: &["signal-set"],
        category: "posix",
        example: "(os/sig-watch |:sigterm :sigint|)",
        effect: RegionEffect::Fresh,
    }
    "os/sig-next" => prim_sig_next {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Wait for the next batch of signal deliveries on a receiver. Yields to the scheduler. Resumes with an array of [{:signal :sigterm :sender-pid n :sender-uid n :code n :count n} ...].",
        params: &["receiver"],
        category: "posix",
        example: "(os/sig-next r)",
        // Opaque: stores nothing (receiver read, events parsed out), but the
        // event array is minted at completion on the origin heap (SigNext
        // completion), neither this call's region nor an arg's. No clique.
        effect: RegionEffect::Opaque,
    }
    "os/sig-close" => prim_sig_close {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a signal receiver. Decrements the watched-set refcount; the signal is unblocked process-wide when the last watcher releases it. Idempotent.",
        params: &["receiver"],
        category: "posix",
        example: "(os/sig-close r)",
        effect: RegionEffect::Immediate,
    }
    "os/sig-pending" => prim_sig_pending {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a set of keywords for signals currently pending delivery on the calling thread (sigpending(2)).",
        category: "posix",
        example: "(os/sig-pending)",
        effect: RegionEffect::Fresh,
    }
    "os/sig-mask" => prim_sig_mask {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a set of keywords for signals currently blocked on the calling thread (pthread_sigmask).",
        category: "posix",
        example: "(os/sig-mask)",
        effect: RegionEffect::Fresh,
    }
    "os/sig-watching" => prim_sig_watching {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return a set of keywords for signals currently being watched by at least one live receiver.",
        category: "posix",
        example: "(os/sig-watching)",
        effect: RegionEffect::Fresh,
    }
}

// Suppress dead-code warning for the SIG_OS_SIGNAL re-export now that
// posix.rs is the only caller. Keep `pub use` so the capability bit is
// visible to anyone who imports it.
#[allow(dead_code)]
pub(crate) const _SIG_OS_SIGNAL_USED: SignalBits = SIG_OS_SIGNAL;
