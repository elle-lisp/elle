use super::*;
use crate::primitives::ctx::NativeCtx;

/// Lower the incoming count the send bumped, now that this message has left the
/// channel buffer — the receive half of the genuinely-Shared (class 7) message's
/// incoming-count accounting (docs/impl/region-model.md § "Why this is hybrid").
///
/// `chan/send` is `RegionEffect::Sends` (docs/impl/region-effects.md § `Sends`): the
/// message crosses the fiber frontier, so it can never be Owned and stays on the
/// per-region RC path, and the `Sends` edge increfs its region at the send site to
/// keep it alive in the buffer *until received* — "a store into a Shared region bumps
/// its count". A receive removes the message from the buffer, so its region's
/// incoming count must drop by one — the matching "an overwrite/drop lowers it". Call
/// this once per message pulled from the buffer, AFTER the `[:ok msg]`/`[i msg]`
/// result carrying it is built, so that result's own reference holds the message
/// across the release (releasing first could free it under the read).
///
/// Guarded by `value_in_region_store`: a cross-thread message (an `os/spawn` worker
/// sending by pointer) lives in the sending thread's heap — a foreign borrow this
/// store neither counts nor may free, so its send-side incref lands on that heap and
/// is balanced there, never here. An immediate message has no region and no-ops.
fn release_received_message(ctx: &mut NativeCtx, msg: Value) {
    let heap = ctx.heap_mut();
    if heap.value_in_region_store(msg) {
        let region = crate::value::arena::region_of(heap, msg);
        crate::value::arena::decref_region(heap, region);
    }
}

/// Helper: extract `&ChanSender` from a Value or return a type error.
pub(super) fn extract_sender<'a>(
    value: &'a Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<&'a ChanSender, (SignalBits, Value)> {
    value.as_external::<ChanSender>().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: expected chan/sender, got {}",
                    prim_name,
                    value.external_type_name().unwrap_or(value.type_name())
                ),
            ),
        )
    })
}

/// Helper: extract `&ChanReceiver` from a Value or return a type error.
pub(super) fn extract_receiver<'a>(
    value: &'a Value,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<&'a ChanReceiver, (SignalBits, Value)> {
    value.as_external::<ChanReceiver>().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: expected chan/receiver, got {}",
                    prim_name,
                    value.external_type_name().unwrap_or(value.type_name())
                ),
            ),
        )
    })
}

/// Validate that `arg` is a non-empty array whose every element is a
/// `chan/receiver` Value, then invoke `f` with a slice of refs to each
/// underlying `ChanReceiver`.  Errors short-circuit out; otherwise the
/// closure's result is returned.
///
/// Both `chan/try-select` and `chan/wait-ready`'s post-register re-check
/// need the same validation + receiver-slice prep, but each then
/// borrows the inner `Option<Receiver>` cells slightly differently
/// (try-select errors on closed, wait-ready falls through to yield).
/// Passing a closure keeps the borrow lifetimes self-contained.
pub(super) fn with_receivers<R>(
    arg: &Value,
    op_name: &str,
    ctx: &mut NativeCtx,
    f: impl FnOnce(&[&ChanReceiver], &mut NativeCtx) -> R,
) -> Result<R, (SignalBits, Value)> {
    let cell = arg.as_array_mut().ok_or_else(|| {
        (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: expected array of receivers, got {}",
                    op_name,
                    arg.type_name()
                ),
            ),
        )
    })?;
    let vec = cell.borrow();
    if vec.is_empty() {
        return Err((
            SIG_ERROR,
            ctx.error(
                "value-error",
                format!("{}: receivers array is empty", op_name),
            ),
        ));
    }
    let mut recvs: Vec<&ChanReceiver> = Vec::with_capacity(vec.len());
    for (i, val) in vec.iter().enumerate() {
        let cr = val.as_external::<ChanReceiver>().ok_or_else(|| {
            (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!(
                        "{}: element {} is not a chan/receiver, got {}",
                        op_name,
                        i,
                        val.external_type_name().unwrap_or(val.type_name())
                    ),
                ),
            )
        })?;
        recvs.push(cr);
    }
    Ok(f(&recvs, ctx))
}

/// `(chan)` or `(chan capacity)`
///
/// Returns `[sender receiver]` as an array.
pub(super) fn prim_chan_new(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let (tx, rx) = if args.is_empty() {
        crossbeam_channel::unbounded()
    } else {
        let cap = match args[0].as_int() {
            Some(n) if n >= 0 => n as usize,
            Some(n) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "value-error",
                        format!("chan: capacity must be non-negative, got {}", n),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "chan: expected integer for capacity, got {}",
                            args[0].type_name()
                        ),
                    ),
                );
            }
        };
        crossbeam_channel::bounded(cap)
    };

    let wake = WakeList::new();
    let sender = ctx.external(
        "chan/sender",
        ChanSender(RefCell::new(Some(tx)), Arc::clone(&wake)),
    );
    let receiver = ctx.external("chan/receiver", ChanReceiver(RefCell::new(Some(rx)), wake));
    (SIG_OK, ctx.array(vec![sender, receiver]))
}

/// `(chan/send sender msg)` — non-blocking send.
///
/// Returns `[:ok]`, `[:full]`, or `[:disconnected]`.
pub(super) fn prim_chan_send(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let sender = match extract_sender(&args[0], "chan/send", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let inner = sender.0.borrow();
    let tx = match inner.as_ref() {
        Some(tx) => tx,
        None => return (SIG_OK, ctx.array(vec![Value::keyword("disconnected")])),
    };

    let result = tx.try_send(SendableValue(args[1]));
    match result {
        Ok(()) => {
            sender.1.wake_all();
            (SIG_OK, ctx.array(vec![Value::keyword("ok")]))
        }
        Err(TrySendError::Full(_)) => (SIG_OK, ctx.array(vec![Value::keyword("full")])),
        Err(TrySendError::Disconnected(_)) => {
            (SIG_OK, ctx.array(vec![Value::keyword("disconnected")]))
        }
    }
}

/// `(chan/recv receiver)` — non-blocking receive.
///
/// Returns `[:ok msg]`, `[:empty]`, or `[:disconnected]`.
pub(super) fn prim_chan_recv(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let receiver = match extract_receiver(&args[0], "chan/recv", ctx) {
        Ok(r) => r,
        Err(e) => return e,
    };

    let inner = receiver.0.borrow();
    let rx = match inner.as_ref() {
        Some(rx) => rx,
        None => return (SIG_OK, ctx.array(vec![Value::keyword("disconnected")])),
    };

    match rx.try_recv() {
        Ok(SendableValue(v)) => {
            let result = ctx.array(vec![Value::keyword("ok"), v]);
            release_received_message(ctx, v);
            (SIG_OK, result)
        }
        Err(TryRecvError::Empty) => (SIG_OK, ctx.array(vec![Value::keyword("empty")])),
        Err(TryRecvError::Disconnected) => {
            (SIG_OK, ctx.array(vec![Value::keyword("disconnected")]))
        }
    }
}

/// `(chan/clone sender)` — clone the sender half.
pub(super) fn prim_chan_clone(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let sender = match extract_sender(&args[0], "chan/clone", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let inner = sender.0.borrow();
    match inner.as_ref() {
        Some(tx) => {
            let cloned = tx.clone();
            (
                SIG_OK,
                ctx.external(
                    "chan/sender",
                    ChanSender(RefCell::new(Some(cloned)), Arc::clone(&sender.1)),
                ),
            )
        }
        None => (
            SIG_ERROR,
            ctx.error("state-error", "chan/clone: sender is closed"),
        ),
    }
}

/// `(chan/close sender)` — close the sender half.
///
/// Drops the inner `Sender`, disconnecting the channel from this end.
/// Wakes any parked `chan/select` so it observes `[:disconnected]` once
/// every sender clone is gone (crossbeam reports the channel as
/// disconnected only after the last sender drops).
pub(super) fn prim_chan_close(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let sender = match extract_sender(&args[0], "chan/close", ctx) {
        Ok(s) => s,
        Err(e) => return e,
    };

    sender.0.borrow_mut().take();
    sender.1.wake_all();
    (SIG_OK, Value::NIL)
}

/// `(chan/close-recv receiver)` — close the receiver half.
///
/// Drops the inner `Receiver`, disconnecting the channel from this end.
/// Wakes any parked `chan/select` so it observes `[:disconnected]`.
pub(super) fn prim_chan_close_recv(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let receiver = match extract_receiver(&args[0], "chan/close-recv", ctx) {
        Ok(r) => r,
        Err(e) => return e,
    };

    receiver.0.borrow_mut().take();
    receiver.1.wake_all();
    (SIG_OK, Value::NIL)
}

/// `(chan/try-select receivers)` — non-blocking poll over receivers.
///
/// Returns `[index msg]` if some receiver has a value ready right now,
/// `[:empty]` if none are ready, or `[:disconnected]` if the ready
/// receiver was observed disconnected.  Errors if any receiver in the
/// array is already closed (via `chan/close-recv`).  Never yields and
/// never blocks — this is the building block the Lisp-level
/// `chan/select` uses to retry after a `chan/wait-ready` wake.
pub(super) fn prim_chan_try_select(
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

/// `(chan/wait-ready receivers)` / `(chan/wait-ready receivers timeout-ms)`
///
/// Park the current fiber until any receiver in `receivers` is signaled
/// by a `chan/send` (or sender/receiver close), or until `timeout-ms`
/// elapses.  Three possible returns:
///
/// - `[:ready index msg]` — fast path: after registering the wake fd in
///   every receiver's `WakeList`, a final `try_select` saw a value
///   already in the channel.  No yield happened; the caller can use
///   the returned `index`/`msg` directly without calling
///   `chan/try-select`.
/// - `[:disconnected]` — same fast path, but the ready receiver was
///   disconnected.
/// - `nil` — the primitive yielded; the fiber was parked on the wake
///   fd until POLLIN or timeout fired.  Caller must follow up with
///   `chan/try-select` to actually pick a ready receiver (and re-park
///   with the remaining timeout if the wake turned out to be spurious;
///   the Lisp `chan/select` wrapper handles this).
///
/// Allocates one wake fd (eventfd on Linux, pipe2 elsewhere) and
/// registers it in every receiver's `WakeList`.  A successful
/// `chan/send` on any of those channels writes a wake byte; the
/// scheduler observes POLLIN via `IORING_OP_POLL_ADD` (or `poll(2)` on
/// the thread-pool backend) and resumes this fiber.  The
/// `ChanSelectGuard` carried by the IoRequest deregisters and closes
/// the fd on completion, cancellation, or aborted submission.
pub(super) fn prim_chan_wait_ready(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Parse timeout before any allocation so a bad timeout cleans up
    // nothing.  nil/missing means wait forever.
    let timeout = if args.len() == 2 && !args[1].is_nil() {
        match args[1].as_int() {
            Some(ms) if ms >= 0 => Some(Duration::from_millis(ms as u64)),
            Some(ms) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "value-error",
                        format!("chan/wait-ready: timeout must be non-negative, got {}", ms),
                    ),
                );
            }
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "chan/wait-ready: expected integer for timeout, got {}",
                            args[1].type_name()
                        ),
                    ),
                );
            }
        }
    } else {
        None
    };

    match with_receivers(&args[0], "chan/wait-ready", ctx, |recvs, ctx| {
        let wake_lists: Vec<Arc<WakeList>> = recvs.iter().map(|r| Arc::clone(&r.1)).collect();

        let (poll_fd, wake_fd) = match make_wake_fd() {
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

        if let Some(result) = recheck {
            let _guard = ChanSelectGuard {
                poll_fd,
                wake_fd,
                wake_lists,
            };
            return (SIG_OK, result);
        }

        let guard = ChanSelectGuard {
            poll_fd,
            wake_fd,
            wake_lists,
        };
        let cell = ChanSelectGuardCell::new(guard);
        let req = IoRequest::with_timeout(ctx, IoOp::ChanSelectPark(cell), Value::NIL, timeout);
        (SIG_YIELD | SIG_IO, req)
    }) {
        Ok(v) => v,
        Err(e) => e,
    }
}
