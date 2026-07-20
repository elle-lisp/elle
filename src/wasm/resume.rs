//! Fiber resume chain: drive suspended WASM closures through yield-resume cycles.

use wasmtime::*;

use super::host::ElleHost;
use super::store::{call_wasm_closure, resume_wasm_closure};
use crate::value::Value;

/// Read signal_bits from the front (innermost) suspension frame for a fiber.
/// The innermost frame carries the original signal (e.g. SIG_IO); outer frames
/// only have SIG_YIELD.
fn front_frame_signal(caller: &Caller<'_, ElleHost>) -> u64 {
    caller
        .data()
        .first_suspension_frame()
        .map(|f| f.signal_bits)
        .unwrap_or(crate::value::fiber::SIG_YIELD.raw())
}

/// The real signal bits a fiber's SUSPEND carries, given the `signal` word a
/// closure call/resume returned. A closure signals a suspend by setting the
/// SIG_YIELD bit; two shapes reach here:
///
/// - **Pure `SIG_YIELD`** (the `rt_yield` suspension path) — a call in the body
///   suspended and pushed a frame; the frame carries the original signal
///   (SIG_IO / SIG_WAIT), so read it from the front frame.
/// - **`SIG_YIELD` set alongside another bit** — a tail-position io returned
///   through the function epilogue (`handle_wasm_result` OR-ed SIG_YIELD onto the
///   native's SIG_IO). No frame was pushed, so `front_frame_signal` would
///   mis-default to SIG_YIELD; the real signal IS `signal` itself.
///
/// Keeping SIG_IO visible here is what lets the scheduler submit the io: a fiber
/// whose final action is a tail `println`/`port/write` (the whole-program thunk
/// under `ev/run` ends this way) would otherwise be mis-routed. Pinned by
/// tests/elle/wasm-tail-io-in-fiber.lisp.
fn yield_sig_bits(caller: &Caller<'_, ElleHost>, signal: i64) -> u64 {
    if signal as u64 == crate::value::fiber::SIG_YIELD.raw() {
        front_frame_signal(caller)
    } else {
        signal as u64
    }
}

/// Resume outcome from drive_resume_chain.
enum ResumeOutcome {
    Dead(Value),
    Yielded(i64, i64, u64),
    Error(i64, i64, u64),
}

/// Drive the resume chain to completion or next yield.
///
/// Repeatedly resumes suspension frames (innermost first) until either:
/// - A frame yields again → Yielded
/// - A frame errors → Error
/// - All frames are consumed → Dead
///
/// When a resumed frame yields again (instead of completing), any
/// remaining old outer frames from the previous yield are stale —
/// the yield-through mechanism already pushed new outer frames for
/// the new yield point. We evict the stale frames so the next
/// resume starts from the new innermost frame.
fn drive_resume_chain(caller: &mut Caller<'_, ElleHost>, initial_value: Value) -> ResumeOutcome {
    let yield_signal = crate::value::fiber::SIG_YIELD.raw() as i64;
    let mut result_val = initial_value;

    loop {
        if !caller.data().has_suspension_frames() {
            return ResumeOutcome::Dead(result_val);
        }
        // Record frame count before resume. If the resumed frame
        // re-yields, new frames are pushed to the back of the deque
        // while old outer frames remain at the front. We need to
        // rotate: move old outer frames behind the new ones so the
        // new inner chain is consumed first on the next resume.
        let frames_before = caller.data().suspension_frame_count();
        match resume_wasm_closure(caller, result_val) {
            Some((t, p, s)) => {
                if s & yield_signal != 0 {
                    if s == yield_signal {
                        // Re-yield via `rt_yield`: a call in the resumed body
                        // suspended again and pushed new frames to the back. After
                        // pop in resume_wasm_closure, old outer frames are at
                        // positions 0..remaining, new frames after — rotate the old
                        // ones behind so the new inner chain resumes first.
                        let remaining_old = frames_before.saturating_sub(1);
                        for _ in 0..remaining_old {
                            if let Some(frame) = caller.data_mut().pop_suspension_frame() {
                                caller.data_mut().push_suspension_frame(frame);
                            }
                        }
                    }
                    // Otherwise SIG_YIELD rode out alongside SIG_IO on a
                    // tail-position io returned through the epilogue: no new frame,
                    // so no rotation, and `yield_sig_bits` reads the signal from `s`
                    // rather than the (absent) front frame.
                    let sig_bits = yield_sig_bits(caller, s);
                    return ResumeOutcome::Yielded(t, p, sig_bits);
                } else if s != 0 {
                    return ResumeOutcome::Error(t, p, s as u64);
                }
                result_val = caller.data().wasm_to_value(t, p);
            }
            None => {
                return ResumeOutcome::Dead(result_val);
            }
        }
    }
}

/// Install `(bits, value)` as the fiber's parked signal and, for a TERMINAL
/// signal, take the same park-retain + record the same `fiber → signal` content
/// edge the VM's fiber driver takes at `with_child_fiber` step 6a
/// (`record_terminal_signal_park`). `handle_fiber_resume` drives fiber bodies
/// outside the VM loop and would otherwise set `fiber.signal` with no
/// bookkeeping, so the host outgoing-edge table drifts from the symmetric release
/// `prim_fiber_resume` runs at the next resume — an over-free / unrecorded-edge
/// panic pinned by `tests/elle/fiber-error-resume.lisp` under `--wasm=full`.
fn install_signal(
    caller: &mut Caller<'_, ElleHost>,
    fiber_handle: &crate::value::FiberHandle,
    fiber_value: Value,
    status: crate::value::FiberStatus,
    bits: crate::value::SignalBits,
    value: Value,
) {
    fiber_handle.with_mut(|f| {
        f.status = status;
        f.signal = Some((bits, value));
    });
    let heap = unsafe { &mut *caller.data().heap_ptr() };
    crate::vm::fiber::record_terminal_signal_park(heap, fiber_value, &Some((bits, value)));
}

/// Route a fiber body's SUSPEND (emit/yield) through the fiber's OWN mask, the
/// way the VM's fiber driver does. Only ERROR routing changes; an ordinary yield
/// / io suspension keeps the unchanged `:paused` + signal-0 path, so the
/// scheduler's parked-signal io detection is untouched.
///
/// - A `SIG_ERROR` the fiber's mask does NOT cover → the fiber goes `:error` and
///   the error is returned as the signal, so the resumer's body re-raises it and
///   the RESUMER's mask is checked one level up the resume chain — the piece the
///   WASM tier's nested-`handle_fiber_resume` recursion otherwise skipped, which
///   left an uncaught `(emit :error …)` wrongly `:paused`.
/// - Anything else (a covered `SIG_ERROR`, a plain yield, an io request) → the
///   fiber pauses and the value flows back to the resumer as a normal result.
///
/// The fiber keeps its suspension frames either way — an `:error` fiber is
/// resumable via the restarts system (tests/elle/fiber-error-resume.lisp).
fn route_emit(
    caller: &mut Caller<'_, ElleHost>,
    fiber_handle: &crate::value::FiberHandle,
    fiber_value: Value,
    tag: i64,
    payload: i64,
    bits: crate::value::SignalBits,
    value: Value,
) -> (i64, i64, i64) {
    let uncaught =
        bits.contains(crate::value::SIG_ERROR) && !fiber_handle.with(|f| f.mask).covers(bits);
    if uncaught {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Error,
            bits,
            value,
        );
        (tag, payload, bits.raw() as i64)
    } else {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Paused,
            bits,
            value,
        );
        (tag, payload, 0)
    }
}

/// Route an ERROR that propagated up into this fiber's body from a resumed child
/// (the child's uncaught error re-raised through this fiber's `fiber/resume`
/// call) through this fiber's OWN mask. A `SIG_ERROR` this mask covers is CAUGHT:
/// the fiber pauses holding it and the value flows back as a normal result
/// (signal 0), so the resumer continues — the WASM analogue of the VM
/// trampoline's caught arm. Anything else keeps the prior propagate behavior:
/// the fiber goes `:error` and `raw_signal` is re-returned to unwind further.
fn route_error(
    caller: &mut Caller<'_, ElleHost>,
    fiber_handle: &crate::value::FiberHandle,
    fiber_value: Value,
    tag: i64,
    payload: i64,
    bits: crate::value::SignalBits,
    value: Value,
) -> (i64, i64, i64) {
    let caught =
        bits.contains(crate::value::SIG_ERROR) && fiber_handle.with(|f| f.mask).covers(bits);
    if caught {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Paused,
            bits,
            value,
        );
        (tag, payload, 0)
    } else {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Error,
            bits,
            value,
        );
        // The unwind signal re-returned to the caller IS this fiber's bits (the
        // callers pass `SignalBits::new(signal)` and `signal` in lockstep).
        (tag, payload, bits.raw() as i64)
    }
}

/// When `fiber/resume` returns SIG_RESUME, the fiber value contains the
/// fiber to execute. We extract it, run its WASM closure, update status.
pub(super) fn handle_fiber_resume(
    caller: &mut Caller<'_, ElleHost>,
    fiber_value: Value,
) -> (i64, i64, i64) {
    use crate::value::fiber::{FiberStatus, SignalBits, SIG_ERROR, SIG_OK, SIG_YIELD};

    let fiber_handle = match fiber_value.as_fiber() {
        Some(f) => f.clone(),
        None => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("type-error", "fiber/resume: not a fiber");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return (tag, payload, SIG_ERROR.raw() as i64);
        }
    };

    let (closure, resume_value, status) = fiber_handle.with_mut(|fiber| {
        let closure = fiber.closure.clone();
        let resume_value = fiber.signal.take().map(|(_, v)| v).unwrap_or(Value::NIL);
        let status = fiber.status;
        (closure, resume_value, status)
    });

    let wasm_idx = match closure.template.wasm_func_idx {
        Some(idx) => idx,
        None => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Error);
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", "fiber/resume: bytecode closure in WASM");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return (tag, payload, SIG_ERROR.raw() as i64);
        }
    };

    let yield_signal = SIG_YIELD.raw() as i64;
    let fiber_id = fiber_handle.id();

    match status {
        FiberStatus::New => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Alive);

            let args = if resume_value.is_nil() {
                vec![]
            } else {
                vec![resume_value]
            };
            caller.data_mut().fiber_id_stack.push(fiber_id);
            // The fiber's root closure is the executing self for its body. A plain
            // thunk `(fn [] …)` has no self-reference, but a self-recursive fiber root
            // resolves `LoadSelf` to itself — build its value from the closure.
            let self_val = {
                let heap = unsafe { &mut *caller.data().heap_ptr() };
                crate::primitives::ctx::Alloc::new(heap).closure((*closure).clone())
            };
            let (self_tag, self_payload) = caller.data_mut().value_to_wasm(self_val);
            let (tag, payload, signal) =
                call_wasm_closure(caller, &closure, wasm_idx, &args, self_tag, self_payload);

            if signal & yield_signal != 0 {
                let yielded = caller.data().wasm_to_value(tag, payload);
                let sig_bits = yield_sig_bits(caller, signal);
                if caller.data().debug {
                    eprintln!(
                        "[handle_fiber_resume] New yield: sig_bits={} (SIG_IO={})",
                        sig_bits,
                        sig_bits & 512
                    );
                }
                let ret = route_emit(
                    caller,
                    &fiber_handle,
                    fiber_value,
                    tag,
                    payload,
                    SignalBits::new(sig_bits),
                    yielded,
                );
                caller.data_mut().fiber_id_stack.pop();
                ret
            } else if signal != 0 {
                let err_val = caller.data().wasm_to_value(tag, payload);
                let ret = route_error(
                    caller,
                    &fiber_handle,
                    fiber_value,
                    tag,
                    payload,
                    SignalBits::new(signal as u64),
                    err_val,
                );
                caller.data_mut().fiber_id_stack.pop();
                ret
            } else {
                let ret_val = caller.data().wasm_to_value(tag, payload);
                install_signal(
                    caller,
                    &fiber_handle,
                    fiber_value,
                    FiberStatus::Dead,
                    SIG_OK,
                    ret_val,
                );
                caller.data_mut().fiber_id_stack.pop();
                (tag, payload, 0)
            }
        }
        // An `:error` fiber is resumable via the restarts system (only `:dead` is
        // terminal), and its suspension frames still live on this store — resume
        // it exactly like a `:paused` one.
        FiberStatus::Paused | FiberStatus::Error => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Alive);
            caller.data_mut().fiber_id_stack.push(fiber_id);

            let outcome = drive_resume_chain(caller, resume_value);
            let ret = match outcome {
                ResumeOutcome::Yielded(t, p, sig_bits) => {
                    let yielded = caller.data().wasm_to_value(t, p);
                    route_emit(
                        caller,
                        &fiber_handle,
                        fiber_value,
                        t,
                        p,
                        SignalBits::new(sig_bits),
                        yielded,
                    )
                }
                ResumeOutcome::Error(t, p, s) => {
                    let err_val = caller.data().wasm_to_value(t, p);
                    route_error(
                        caller,
                        &fiber_handle,
                        fiber_value,
                        t,
                        p,
                        SignalBits::new(s),
                        err_val,
                    )
                }
                ResumeOutcome::Dead(result_val) => {
                    install_signal(
                        caller,
                        &fiber_handle,
                        fiber_value,
                        FiberStatus::Dead,
                        SIG_OK,
                        result_val,
                    );
                    let (t, p) = caller.data_mut().value_to_wasm(result_val);
                    (t, p, 0)
                }
            };

            caller.data_mut().fiber_id_stack.pop();
            ret
        }
        _ => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("fiber-error", "fiber/resume: fiber not resumable");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, SIG_ERROR.raw() as i64)
        }
    }
}
