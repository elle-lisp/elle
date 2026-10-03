// audited: 2026-09-30
//! The select primitives: poll a set of receivers, or park the fiber until one is ready.
//!
//! docs/threads.md
//! docs/io/timeout.md

use super::*;
use crate::primitives::kwarg::extract_bound;

/// `(chan/try-select receivers)` — non-blocking poll over receivers.
///
/// Returns `[index msg]` if some receiver has a value ready right now,
/// `[:empty]` if none are ready, or `[:disconnected]` if the ready
/// receiver was observed disconnected.  Errors if any receiver in the
/// array is already closed (via `chan/close-recv`).  Never yields and
/// never blocks — this is the building block the Lisp-level
/// `chan/select` uses to retry after a `chan/wait-ready` wake.
pub(in crate::primitives::chan) fn prim_chan_try_select(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    match with_receivers(&args[0], "chan/try-select", ctx, |recvs, ctx| {
        let borrows: Vec<_> = recvs.iter().map(|r| r.0.borrow()).collect();
        let mut sel = crossbeam_channel::Select::new();
        let mut rxs: Vec<&crossbeam_channel::Receiver<SendableValue>> =
            Vec::with_capacity(borrows.len());
        for (i, b) in borrows.iter().enumerate() {
            match b.as_ref() {
                Some(rx) => {
                    rxs.push(rx);
                    sel.recv(rx);
                }
                None => {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "state-error",
                            format!("chan/try-select: receiver at index {} is closed", i),
                        ),
                    );
                }
            }
        }
        // Bind so the SelectedOperation temporary is dropped before
        // `borrows` at the end of the closure scope.
        let outcome = match sel.try_select() {
            Ok(oper) => {
                let index = oper.index();
                match oper.recv(rxs[index]) {
                    Ok(SendableValue(v)) => {
                        let result = ctx.array(vec![Value::int(index as i64), v]);
                        release_received_message(ctx, v);
                        (SIG_OK, result)
                    }
                    Err(_) => (SIG_OK, ctx.array(vec![Value::keyword("disconnected")])),
                }
            }
            Err(_) => (SIG_OK, ctx.array(vec![Value::keyword("empty")])),
        };
        outcome
    }) {
        Ok(v) => v,
        Err(e) => e,
    }
}

/// `(chan/wait-ready receivers &named timeout deadline)`
///
/// Park the current fiber until any receiver in `receivers` is signaled
/// by a `chan/send` (or sender/receiver close), or until its bound passes
/// (docs/io/timeout.md).  Three possible returns:
///
/// - `[:ready index msg]` — fast path: after registering the wake fd in
///   every receiver's `WakeList`, a final `try_select` saw a value
///   already in the channel.  No yield happened; the caller can use
///   the returned `index`/`msg` directly without calling
///   `chan/try-select`.
/// - `[:disconnected]` — same fast path, but the ready receiver was
///   disconnected.
/// - `nil` — the primitive yielded; the fiber was parked on the wake
///   fd until POLLIN or the bound fired.  Caller must follow up with
///   `chan/try-select` to actually pick a ready receiver (and re-park
///   if the wake turned out to be spurious; the Lisp `chan/select`
///   wrapper handles this).
///
/// Allocates one wake fd (eventfd on Linux, pipe2 elsewhere) and
/// registers it in every receiver's `WakeList`.  A successful
/// `chan/send` on any of those channels writes a wake byte; the
/// scheduler observes POLLIN via `IORING_OP_POLL_ADD` (or `poll(2)` on
/// the thread-pool backend) and resumes this fiber.  The
/// `ChanSelectGuard` carried by the IoRequest deregisters and closes
/// the fd on completion, cancellation, or aborted submission.
pub(in crate::primitives::chan) fn prim_chan_wait_ready(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Parse the bound before any allocation so a bad bound cleans up
    // nothing.  No bound means wait forever.
    let bound = match extract_bound(args, 1, "chan/wait-ready", ctx) {
        Ok(b) => b,
        Err(e) => return e,
    };

    match with_receivers(&args[0], "chan/wait-ready", ctx, |recvs, ctx| {
        // The selecting instance's trace cell — carried into the wake fds and the
        // park guard so their `chan_trace` lines gate on this instance's trace.
        let trace = ctx.heap_mut().trace_cell();
        let wake_lists: Vec<Arc<WakeList>> = recvs.iter().map(|r| Arc::clone(&r.1)).collect();

        let (poll_fd, wake_fd) = match make_wake_fd(&trace) {
            Ok(pair) => pair,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "io-error",
                        format!("chan/wait-ready: failed to allocate wake fd: {}", e),
                    ),
                );
            }
        };

        // Register the *wake* fd in every receiver's wake list — the
        // write-side fd (same as poll_fd on Linux's eventfd, distinct
        // on pipe-based platforms).  Doing this *before* the
        // post-register re-check below means any send happening from
        // this moment on writes to our wake fd (counter semantics on
        // eventfd, byte-buffer semantics on pipe), so the upcoming
        // POLL_ADD / poll(2) returns POLLIN immediately even if the
        // kernel hasn't yet armed the poll when the send fires.
        for wl in &wake_lists {
            wl.register(wake_fd);
        }

        // Close the cross-thread race window between the wrapper's first
        // chan/try-select and this register: a send that snuck in
        // between (with an empty wake-list and therefore no signal) is
        // still observed by this re-check.  If we find something
        // ready, do not yield — extract the value and return [:ready i
        // v] so the caller can skip its own chan/try-select call.  A
        // closed receiver here falls through to the yield path; the
        // wake from chan/close-recv will unblock us promptly and the
        // wrapper's chan/try-select reports the closure.
        //
        // Done inside an inner block so the borrows / Select / rxs all
        // drop before we either build the guard early (fast return) or
        // hand it to the yield IoRequest.
        let recheck: Option<Value> = {
            let borrows: Vec<_> = recvs.iter().map(|r| r.0.borrow()).collect();
            let mut sel = crossbeam_channel::Select::new();
            let mut rxs: Vec<&crossbeam_channel::Receiver<SendableValue>> =
                Vec::with_capacity(borrows.len());
            let mut all_open = true;
            for b in borrows.iter() {
                match b.as_ref() {
                    Some(rx) => {
                        rxs.push(rx);
                        sel.recv(rx);
                    }
                    None => {
                        all_open = false;
                        break;
                    }
                }
            }
            if all_open {
                match sel.try_select() {
                    Ok(oper) => {
                        let index = oper.index();
                        Some(match oper.recv(rxs[index]) {
                            Ok(SendableValue(v)) => {
                                let result = ctx.array(vec![
                                    Value::keyword("ready"),
                                    Value::int(index as i64),
                                    v,
                                ]);
                                release_received_message(ctx, v);
                                result
                            }
                            Err(_) => ctx.array(vec![Value::keyword("disconnected")]),
                        })
                    }
                    Err(_) => None,
                }
            } else {
                None
            }
        };

        let guard = ChanSelectGuard::new(poll_fd, wake_fd, wake_lists, trace);
        if let Some(result) = recheck {
            // Nothing parks, so the guard's drop deregisters the wake fd and
            // closes the pair now.
            drop(guard);
            return (SIG_OK, result);
        }

        let cell = ChanSelectGuardCell::new(guard);
        let req = IoRequest::bounded(ctx, IoOp::ChanSelectPark(cell), Value::NIL, bound);
        (SIG_IO, req)
    }) {
        Ok(v) => v,
        Err(e) => e,
    }
}
