// audited: 2026-09-21
// docs/impl/wasm.md
//! What a fiber's OWN mask decides about the outcome its body produced, and the
//! bookkeeping that installing the resulting signal owes.
//!
//! Split from the chain driver beside it because the two answer different
//! questions. `resume.rs` decides which frame runs next; this decides what the
//! fiber's mask makes of the answer that frame gave — caught, parked, or
//! propagated one level up the resume chain.

use wasmtime::*;

use crate::value::Value;
use crate::wasm::host::ElleHost;
use crate::wasm::outcome::CallOutcome;

/// Install `(bits, value)` as the fiber's parked signal and, for a TERMINAL
/// signal, take the same park-retain + record the same `fiber → signal` content
/// edge the VM's fiber driver takes at `with_child_fiber` step 6a
/// (`record_terminal_signal_park`). `handle_fiber_resume` drives fiber bodies
/// outside the VM loop and would otherwise set `fiber.signal` with no
/// bookkeeping, so the host outgoing-edge table drifts from the symmetric release
/// `prim_fiber_resume` runs at the next resume — an over-free / unrecorded-edge
/// panic pinned by `tests/elle/fiber-error-resume.lisp` under `--wasm=full`.
pub(super) fn install_signal(
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
pub(super) fn route_emit(
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
        let parent_id = caller.data().fiber_id_stack[stack_len - 2].id;
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
pub(super) fn route_error(
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
