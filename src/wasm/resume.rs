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
                if s == yield_signal {
                    // After pop in resume_wasm_closure, old outer frames
                    // are at positions 0..remaining, new frames after.
                    // Rotate old frames to the back.
                    let remaining_old = frames_before.saturating_sub(1);
                    for _ in 0..remaining_old {
                        if let Some(frame) = caller.data_mut().pop_suspension_frame() {
                            caller.data_mut().push_suspension_frame(frame);
                        }
                    }
                    let sig_bits = front_frame_signal(caller);
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
            let err = crate::value::error_val("type-error", "fiber/resume: not a fiber");
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
            let err =
                crate::value::error_val("internal-error", "fiber/resume: bytecode closure in WASM");
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
            let (tag, payload, signal) = call_wasm_closure(caller, &closure, wasm_idx, &args);

            if signal == yield_signal {
                let yielded = caller.data().wasm_to_value(tag, payload);
                let sig_bits = front_frame_signal(caller);
                if caller.data().debug {
                    eprintln!(
                        "[handle_fiber_resume] New yield: sig_bits={} (SIG_IO={})",
                        sig_bits,
                        sig_bits & 512
                    );
                }
                fiber_handle.with_mut(|f| {
                    f.status = FiberStatus::Paused;
                    f.signal = Some((SignalBits::new(sig_bits), yielded));
                });
                caller.data_mut().fiber_id_stack.pop();
                (tag, payload, 0)
            } else if signal != 0 {
                let err_val = caller.data().wasm_to_value(tag, payload);
                fiber_handle.with_mut(|f| {
                    f.status = FiberStatus::Error;
                    f.signal = Some((SignalBits::new(signal as u64), err_val));
                });
                caller.data_mut().fiber_id_stack.pop();
                (tag, payload, signal)
            } else {
                let ret_val = caller.data().wasm_to_value(tag, payload);
                fiber_handle.with_mut(|f| {
                    f.status = FiberStatus::Dead;
                    f.signal = Some((SIG_OK, ret_val));
                });
                caller.data_mut().fiber_id_stack.pop();
                (tag, payload, 0)
            }
        }
        FiberStatus::Paused => {
            fiber_handle.with_mut(|f| f.status = FiberStatus::Alive);
            caller.data_mut().fiber_id_stack.push(fiber_id);

            let outcome = drive_resume_chain(caller, resume_value);
            let ret = match outcome {
                ResumeOutcome::Yielded(t, p, sig_bits) => {
                    let yielded = caller.data().wasm_to_value(t, p);
                    fiber_handle.with_mut(|f| {
                        f.status = FiberStatus::Paused;
                        f.signal = Some((SignalBits::new(sig_bits), yielded));
                    });
                    (t, p, 0)
                }
                ResumeOutcome::Error(t, p, s) => {
                    let err_val = caller.data().wasm_to_value(t, p);
                    fiber_handle.with_mut(|f| {
                        f.status = FiberStatus::Error;
                        f.signal = Some((SignalBits::new(s), err_val));
                    });
                    (t, p, s as i64)
                }
                ResumeOutcome::Dead(result_val) => {
                    fiber_handle.with_mut(|f| {
                        f.status = FiberStatus::Dead;
                        f.signal = Some((SIG_OK, result_val));
                    });
                    let (t, p) = caller.data_mut().value_to_wasm(result_val);
                    (t, p, 0)
                }
            };

            caller.data_mut().fiber_id_stack.pop();
            ret
        }
        _ => {
            let err = crate::value::error_val("fiber-error", "fiber/resume: fiber not resumable");
            let (tag, payload) = caller.data_mut().value_to_wasm(err);
            (tag, payload, SIG_ERROR.raw() as i64)
        }
    }
}
