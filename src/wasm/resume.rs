//! Fiber resume chain: drive suspended WASM closures through yield-resume cycles.
//!
//! Beyond plain yield/resume, this module mirrors two VM fiber-driver behaviors
//! the host must reproduce because it drives fiber bodies outside the VM loop:
//!
//! - **Uncaught-suspend propagation + re-drive** (`route_emit`, `redrive_child`,
//!   `drive_resume_chain`): when a `(fiber/resume child)` sees `child` suspend on
//!   a scheduler wait/io its mask does not cover, the wait propagates to the
//!   resumer (so the scheduler drives it) and the resumer re-drives `child` on
//!   its own resume. This is what makes `protect`/`defer`/`with` around a
//!   suspending body work — the WASM analogue of the VM's FiberResume frame
//!   (src/vm/fiber/trampoline.rs). Pinned by tests/elle/wasm-protect-suspend.lisp.
//! - **Signal re-raise** (`handle_fiber_propagate`): `(fiber/propagate child)`
//!   re-raises `child`'s caught signal as the caller's own — the WASM analogue of
//!   `handle_fiber_propagate_signal` (src/vm/fiber/propagate.rs), used by
//!   `defer`/`with` to surface a caught body error.

use wasmtime::*;

use super::host::ElleHost;
use super::store::{call_wasm_closure, resume_wasm_closure};
use crate::value::Value;
use crate::wasm::outcome::CallOutcome;

/// Resume outcome from drive_resume_chain.
enum ResumeOutcome {
    Dead(Value),
    Yielded(i64, i64, u64),
    Error(i64, i64, u64),
}

/// Outcome of re-driving a parked child fiber (a `protect`/`defer`/`with` body
/// that propagated an uncaught scheduler wait).
enum RedriveOutcome {
    /// The child completed (or caught its own error): its result value flows to
    /// the frame that awaited it.
    Completed(Value),
    /// The child suspended again on another uncaught wait/io: propagate the
    /// signal so the scheduler drives it, and re-drive the child again next time.
    Suspended(i64, i64, u64),
    /// The child raised an error its mask does not cover: propagate the error.
    Errored(i64, i64, u64),
}

/// Re-drive a parked child fiber with the value the scheduler delivered.
///
/// Mirrors the VM's `FiberResume` arm (src/vm/fiber/resume.rs): it overwrites
/// the child's parked wait signal with `(SIG_OK, resume_value)`, then re-enters
/// the child via `handle_fiber_resume` and classifies the outcome by the returned
/// signal. A nested uncaught suspension re-registers a re-drive against the
/// CURRENT fiber; the awaiting frame already carries that child, so the duplicate
/// pending entry is dropped here.
fn redrive_child(
    caller: &mut Caller<'_, ElleHost>,
    child_value: Value,
    resume_value: Value,
) -> RedriveOutcome {
    if let Some(handle) = child_value.as_fiber() {
        handle.with_mut(|f| f.signal = Some((crate::value::fiber::SIG_OK, resume_value)));
    }
    let current = caller.data().current_fiber_id();
    let out = handle_fiber_resume(caller, child_value);
    caller.data_mut().pending_redrive.remove(&current);

    if out.suspended {
        RedriveOutcome::Suspended(out.tag, out.payload, out.signal.raw())
    } else if !out.signal.is_empty() {
        RedriveOutcome::Errored(out.tag, out.payload, out.signal.raw())
    } else {
        RedriveOutcome::Completed(caller.data().wasm_to_value(out.tag, out.payload))
    }
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
    let mut result_val = initial_value;

    loop {
        if !caller.data().has_suspension_frames() {
            return ResumeOutcome::Dead(result_val);
        }
        // Before resuming the front frame, honour any child re-drive it awaits: a
        // `protect`/`defer`/`with` body that suspended uncaught on a scheduler
        // wait must be driven to completion with the scheduler's value before its
        // resumer's continuation runs. The WASM analogue of the VM's FiberResume
        // frame (src/vm/fiber/trampoline.rs). Loops here until the child is done
        // (its result becomes this frame's resume value) or re-suspends (propagate
        // and re-drive again next time).
        if let Some(child) = caller.data().first_frame_redrive_child() {
            match redrive_child(caller, child, result_val) {
                RedriveOutcome::Completed(v) => {
                    caller.data_mut().clear_first_frame_redrive();
                    result_val = v;
                }
                RedriveOutcome::Suspended(t, p, s) => return ResumeOutcome::Yielded(t, p, s),
                RedriveOutcome::Errored(t, p, s) => return ResumeOutcome::Error(t, p, s),
            }
        }
        // Record frame count before resume. If the resumed frame
        // re-yields, new frames are pushed to the back of the deque
        // while old outer frames remain at the front. We need to
        // rotate: move old outer frames behind the new ones so the
        // new inner chain is consumed first on the next resume.
        let frames_before = caller.data().suspension_frame_count();
        match resume_wasm_closure(caller, result_val) {
            Some(out) => {
                if out.suspended {
                    // Did `rt_yield` push new frames, or did a tail-position io
                    // ride out through the epilogue with no frame at all? Ask the
                    // deque, not the signal word: `resume_wasm_closure` popped one
                    // frame, so any count above `frames_before - 1` is new.
                    let remaining_old = frames_before.saturating_sub(1);
                    if caller.data().suspension_frame_count() > remaining_old {
                        // Re-yield: after the pop, old outer frames sit at
                        // 0..remaining_old and the new ones after them. Rotate the
                        // old ones behind so the new inner chain resumes first.
                        for _ in 0..remaining_old {
                            if let Some(frame) = caller.data_mut().pop_suspension_frame() {
                                caller.data_mut().push_suspension_frame(frame);
                            }
                        }
                    }
                    return ResumeOutcome::Yielded(out.tag, out.payload, out.signal.raw());
                } else if !out.signal.is_empty() {
                    return ResumeOutcome::Error(out.tag, out.payload, out.signal.raw());
                }
                result_val = caller.data().wasm_to_value(out.tag, out.payload);
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
/// way the VM's fiber driver does.
///
/// - A `SIG_ERROR` the fiber's mask does NOT cover → the fiber goes `:error` and
///   the error is returned as the signal, so the resumer's body re-raises it and
///   the RESUMER's mask is checked one level up the resume chain — the piece the
///   WASM tier's nested-`handle_fiber_resume` recursion otherwise skipped, which
///   left an uncaught `(emit :error …)` wrongly `:paused`.
/// - A `SIG_WAIT`/`SIG_IO` suspension the fiber's mask does NOT cover → PROPAGATE
///   it to the resumer, parked (so the resumer's `fiber/resume` SuspendingCall
///   captures a continuation and the wait reaches the scheduler) and carrying the
///   fiber's real bits, not a transport bit OR-ed on top —
///   and register a re-drive of this fiber against the parent, so the parent's
///   next resume feeds the scheduler's value back into this fiber rather than
///   into the parent's continuation. This is the WASM analogue of the VM
///   trampoline's uncaught-suspend arm that builds a `FiberResume` frame on the
///   parent (src/vm/fiber/trampoline.rs); it is what makes `protect`/`defer`/
///   `with` around a suspending body work. Pinned by
///   tests/elle/wasm-protect-suspend.lisp.
/// - Anything else (a covered `SIG_ERROR`/`SIG_WAIT`/`SIG_IO`, a plain yield, a
///   masked io request the scheduler drives explicitly) → the fiber pauses and
///   the value flows back to the resumer as a normal result (signal 0), so the
///   scheduler's parked-signal io detection is untouched.
///
/// The fiber keeps its suspension frames in every case — an `:error` fiber is
/// resumable via the restarts system (tests/elle/fiber-error-resume.lisp), and a
/// propagated fiber must replay its frames when the parent re-drives it.
fn route_emit(
    caller: &mut Caller<'_, ElleHost>,
    fiber_handle: &crate::value::FiberHandle,
    fiber_value: Value,
    tag: i64,
    payload: i64,
    bits: crate::value::SignalBits,
    value: Value,
) -> CallOutcome {
    let mask = fiber_handle.with(|f| f.mask);

    if bits.intersects(crate::value::SIG_ERROR) && !mask.covers(bits) {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Error,
            bits,
            value,
        );
        return CallOutcome::signalled(tag, payload, bits);
    }

    // An uncaught scheduler suspension (wait/io the mask does not cover) must
    // propagate to the resumer so the scheduler drives it — the resumer's mask,
    // one level up, decides where it is finally caught. `bits` is the fiber's
    // own signal, in the vocabulary the VM checks; the tier no longer mixes a
    // transport bit into it.
    let is_scheduler_suspend =
        bits.intersects(crate::signals::SIG_IO.union(crate::signals::SIG_WAIT));
    let stack_len = caller.data().fiber_id_stack.len();
    if is_scheduler_suspend && !mask.covers(bits) && stack_len >= 2 {
        // Park this fiber holding its wait so it stays resumable with its frames;
        // the parent re-drives it (overwriting this signal) on resume.
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Paused,
            bits,
            value,
        );
        // The parent is the fiber directly below us on the resume stack — the one
        // whose `fiber/resume` call is about to observe this yield.
        let parent_id = caller.data().fiber_id_stack[stack_len - 2];
        caller
            .data_mut()
            .pending_redrive
            .insert(parent_id, fiber_value);
        // Park the parent — its `fiber/resume` SuspendingCall must capture a
        // continuation — and carry the fiber's real bits so the parent's own
        // park records the wait/io. `suspended` says park; the bits stay clean,
        // which is what keeps `fiber/bits` reporting |:io| and not |:io :yield|.
        return CallOutcome::parked(tag, payload, bits);
    }

    install_signal(
        caller,
        fiber_handle,
        fiber_value,
        crate::value::FiberStatus::Paused,
        bits,
        value,
    );
    CallOutcome::value(tag, payload)
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
) -> CallOutcome {
    let caught =
        bits.intersects(crate::value::SIG_ERROR) && fiber_handle.with(|f| f.mask).covers(bits);
    if caught {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Paused,
            bits,
            value,
        );
        CallOutcome::value(tag, payload)
    } else {
        install_signal(
            caller,
            fiber_handle,
            fiber_value,
            crate::value::FiberStatus::Error,
            bits,
            value,
        );
        // The unwind signal re-returned to the caller IS this fiber's bits.
        CallOutcome::signalled(tag, payload, bits)
    }
}

/// When `fiber/resume` returns SIG_PROPAGATE, re-raise the named child fiber's
/// caught signal as this call's own signal — the WASM analogue of the VM's
/// `handle_fiber_propagate_signal` (src/vm/fiber/propagate.rs). `defer`/`with`
/// call `(fiber/propagate f)` in tail position of their unwind branch to surface
/// a caught body error; the child's `(bits, value)` becomes this call's result,
/// so the enclosing body unwinds with the real error rather than the fiber value.
pub(super) fn handle_fiber_propagate(
    caller: &mut Caller<'_, ElleHost>,
    fiber_value: Value,
) -> CallOutcome {
    use crate::value::fiber::SIG_ERROR;

    let (child_bits, child_value) = match fiber_value.as_fiber() {
        Some(h) => h.with(|f| f.signal).unwrap_or_else(|| {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            (
                SIG_ERROR,
                ctx.error("internal-error", "fiber/propagate: no signal"),
            )
        }),
        None => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", "fiber/propagate: not a fiber");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return CallOutcome::error(tag, payload);
        }
    };

    let (tag, payload) = caller.data_mut().value_to_wasm(child_value);
    CallOutcome::signalled(tag, payload, child_bits)
}

/// When `fiber/resume` returns SIG_RESUME, the fiber value contains the
/// fiber to execute. We extract it, run its WASM closure, update status.
pub(super) fn handle_fiber_resume(
    caller: &mut Caller<'_, ElleHost>,
    fiber_value: Value,
) -> CallOutcome {
    use crate::value::fiber::{FiberStatus, SignalBits, SIG_OK};

    let fiber_handle = match fiber_value.as_fiber() {
        Some(f) => f.clone(),
        None => {
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("type-error", "fiber/resume: not a fiber");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return CallOutcome::error(tag, payload);
        }
    };

    let (closure, resume_value, status, unfunded) = fiber_handle.with_mut(|fiber| {
        let closure = fiber.closure.clone();
        let resume_value = fiber.signal.take().map(|(_, v)| v).unwrap_or(Value::NIL);
        let status = fiber.status;
        // The park's funding travels with the parked signal, taken through the
        // same delivery funnel the VM's fiber driver uses
        // (`do_fiber_resume_single`), so every delivery route consumes it once
        // and the two tiers cannot drift.
        let unfunded = fiber.delivery.take_resume_funding();
        (closure, resume_value, status, unfunded)
    });

    let wasm_idx = match closure.template.wasm_func_idx {
        Some(idx) => idx,
        None => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Error);
            let heap = unsafe { &mut *caller.data().heap_ptr() };
            let ctx = crate::primitives::ctx::Alloc::new(heap);
            let err = ctx.error("internal-error", "fiber/resume: bytecode closure in WASM");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            return CallOutcome::error(tag, payload);
        }
    };

    // Fund the crossing into a frame parked at a suspending PRIMITIVE call: that
    // frame resumes into the parked call's continuation, which runs the call's
    // compiler-emitted result release, and the primitive that never returned
    // minted nothing for it (docs/impl/region/owner.md § "A delivery into a
    // replayed frame carries one owning reference"). Emitted past the guards
    // above, so a resume that never reaches the body mints nothing; `region_of`
    // no-ops an immediate.
    if unfunded {
        let heap = unsafe { &mut *caller.data().heap_ptr() };
        let r = crate::value::arena::region_of(heap, resume_value);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::ResumeDelivery,
        );
    }

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
            let out = call_wasm_closure(caller, &closure, wasm_idx, &args, self_tag, self_payload);
            let (tag, payload) = (out.tag, out.payload);

            if out.suspended {
                let yielded = caller.data().wasm_to_value(tag, payload);
                if caller.data().debug {
                    eprintln!(
                        "[handle_fiber_resume] New yield: signal={} (SIG_IO={})",
                        out.signal,
                        out.signal.intersects(crate::signals::SIG_IO)
                    );
                }
                let ret = route_emit(
                    caller,
                    &fiber_handle,
                    fiber_value,
                    tag,
                    payload,
                    out.signal,
                    yielded,
                );
                caller.data_mut().fiber_id_stack.pop();
                ret
            } else if !out.signal.is_empty() {
                let err_val = caller.data().wasm_to_value(tag, payload);
                let ret = route_error(
                    caller,
                    &fiber_handle,
                    fiber_value,
                    tag,
                    payload,
                    out.signal,
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
                CallOutcome::value(tag, payload)
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
                    CallOutcome::value(t, p)
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
            CallOutcome::error(tag, payload)
        }
    }
}
