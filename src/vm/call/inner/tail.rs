// audited: 2026-11-13
//! `VM::tail_call_inner`: the TailCall and TailCallArrayMut dispatch both handlers share.
//!
//! docs/impl/vm.md
//! docs/impl/region/handoff.md

use super::*;
use crate::vm::call::stash::TailStash;

impl VM {
    /// Shared TailCall/TailCallArrayMut logic after argument extraction.
    ///
    /// Dispatches a native through the tail signal handler. A closure callee
    /// is counted for the JIT and built into a pending tail call, which carries
    /// its environment or its compiled code (`pending_tail_call_to`).
    ///
    /// When `checked` is true, the compiler verified arity at compile time
    /// and the runtime skips the arity check for primitives and closures.
    ///
    /// `stash.slots` names the frame locals holding this call's
    /// borrowed-argument retains. That retain has exactly one consumer per path:
    /// a frame-replacing closure callee's owned-param release, or the
    /// post-`TailCall` fall-through block, which runs on one outcome only — the
    /// native's normal completion. A SIGNAL exit reaches neither, so it consumes
    /// the retain here instead — except at a SUSPEND, which parks the
    /// continuation at the post-`TailCall` ip, so the block does run on resume and
    /// every retain stands until it does (docs/impl/region/signalexit.md). A
    /// terminal `:error` consumes them like any other exit and mints the
    /// payload's delivery instead ([`Self::mint_raised_argument_delivery`]).
    ///
    /// `spliced_args` says the arguments came out of an args array the calling
    /// convention owns rather than off this frame's own operand stack. That
    /// decides who holds the reference the callee's owned-param release consumes:
    /// the array does, so a spliced tail call MOVES nothing and the callee mints
    /// one reference per parameter of its own (`ArgRefs::tail`), exactly as a
    /// call-position callee does. The caller reclaims the array once this returns,
    /// and the cascade balances against those mints
    /// (docs/impl/region/mechanism.md).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tail_call_inner(
        &mut self,
        func: Value,
        args: Vec<Value>,
        checked: bool,
        region_id: StaticRegion,
        defer_callee_release: bool,
        deferred_release_slot: Option<StaticRegion>,
        stash: TailStash<'_>,
        spliced_args: bool,
    ) -> Option<SignalBits> {
        let borrowed_arg_slots = stash.slots;
        self.stamp_late_mints(stash);
        if let Some(def) = func.as_native_def() {
            let blocked = self.capability_blocked(def, &args);
            if !blocked.is_empty() {
                // The denial never runs the native at all, so the fall-through
                // block is abandoned exactly as it is on any other signal exit.
                // Its park spares nothing: the payload is a struct the denial
                // built, which names no argument of this call.
                let owed = self.take_borrowed_arg_retains(borrowed_arg_slots);
                let bits = self.handle_capability_denial_tail(def, blocked, &args);
                self.run_borrowed_arg_retains(owed);
                return Some(bits);
            }
            if !checked && !self.check_native_arity(def, args.len()) {
                let owed = self.take_borrowed_arg_retains(borrowed_arg_slots);
                self.run_borrowed_arg_retains(owed);
                return Some(SIG_ERROR);
            }
            // Fresh per-execution result region + pass-through retain, shared
            // with the Call-position path and the JIT. (A yielding native's
            // escape retain is handled inside `handle_primitive_signal_tail`'s
            // Suspend arm; a fresh result lives in this call's `alloc_region` and
            // is skipped by `dispatch_native_call`, so the two never double-count.)
            let (bits, value) = self.dispatch_native_call(def, &args, region_id);
            // A native pushes no bytecode frame, so a native tail call that
            // completes normally does not replace the frame. It pushes the result
            // and returns `None`, as the non-tail `Ok` path does, and the dispatch
            // loop continues into the post-`TailCall` block: store the result,
            // release each owned argument, retain the result, `Return`. The
            // compiler emits each argument's owning `DecrefValueRegion` in that
            // block, with its exact per-argument ownership, so a value with a live
            // release elsewhere, stored into the result, or aliased is released
            // once (region-closure-struct.lisp, box.lisp). No runtime rule that
            // releases every argument can match that. Replacing the frame skips
            // the block, and an argument whose last use is this call is then never
            // released (region-native-tail-move.lisp).
            //
            // Two retains on the result region keep the accounting balanced: the
            // `dispatch_native_call` pass-through retain, which the caller's
            // `DecrefValueRegion` consumes, and the block's ReturnValue
            // `IncrefValueRegion` (`lower_call`, the tail mirror of
            // `lower_return`), which keeps the returned value alive for the
            // caller's binding. The ReturnValue retain comes before the
            // owned-argument releases, so a result borrowed from an argument's
            // region outlives that argument's release
            // (region-native-tail-return-uaf.lisp).
            //
            // `bits.is_empty()` is exactly `SignalAction::Ok` (see `classify`). A
            // native that signals may carry an argument inside its value: a
            // yielding `port/write` or `port/flush` hands the scheduler an
            // `IoRequest` that holds the port. That native leaves by the signal
            // path below, which retains the yielded value and releases the
            // argument on resume or abort. Running the block's releases instead
            // would free a port the scheduler still reads.
            if bits.is_empty() {
                self.fiber.stack.push(value);
                return None;
            }
            // The signal exit abandons the fall-through block, so it consumes
            // the borrowed-argument retains that block would have. The NAMES are
            // taken first — before the handler, which may swap fibers under us —
            // and the regions released after it, so a payload that carries one
            // of these values (a fiber carrier's own argument, an `IoRequest`
            // embedding a port) is still held while the handler installs it.
            //
            // A SUSPEND is the exit that does not abandon the block: the driver it
            // unwinds to parks the continuation at the post-`TailCall` ip and the
            // resume replays it. So it takes no retain. The one naming the payload
            // is the reference the body owes for what it yields. Any other may be
            // the last reference to a value the payload embeds without a count,
            // as an `IoRequest` embeds its port and its bytes. The replayed block
            // releases each, and for a park nobody resumes the walk or the
            // discharge runs that release off the frame's table. So the suspend
            // makes every late mint too, and each stash it writes back holds the
            // reference that release consumes (docs/impl/region/handoff.md).
            let action = crate::signals::dispatch::classify(bits, &value);
            let suspends = action == crate::signals::dispatch::SignalAction::Suspend;
            if suspends {
                self.mint_suspended_late(stash, &args);
            }
            let owed =
                self.take_borrowed_arg_retains(if suspends { &[] } else { borrowed_arg_slots });
            // A terminal :error hands the payload to a catcher, whose read
            // consumes one reference the frame's own routes do not fund — so the
            // raise mints it here where the payload is an argument of this call.
            // A HALT takes none for the reason `handle_emit` takes none: the
            // fiber is promoted to `:dead`, so that delivery has no consumer.
            // Minted before the handler, which is where this fiber stops being
            // the one whose frames hold the payload.
            if action == crate::signals::dispatch::SignalAction::Error {
                self.mint_raised_argument_delivery(&args, value);
            }
            let bits = self.handle_primitive_signal_tail(bits, value, &args);
            self.run_borrowed_arg_retains(owed);
            // A fiber CARRIER (`fiber/resume`/`fiber/abort`/`fiber/propagate`/
            // `fiber/refuse`)
            // leaves the primitive as a signal because it is a request: drive
            // this child fiber and report what happened. Where this fiber's own
            // mask ABSORBS the child's outcome the request is answered here, so
            // the value is the call's result and the frame never left — it takes
            // the fall-through, exactly as a native that completed normally does
            // (docs/impl/region/signalexit.md). The post-`TailCall` block then runs
            // the releases it holds for this call: one per owned argument, plus
            // the result's own, plus the return mint. Handing the value out
            // through `fiber.signal` instead reaches none of them, and no other
            // path does either — an absorbed outcome is not an error, so the
            // abandoned-frame walk does not fire, and not a suspend, so no replay
            // arrives. The Call position and the JIT tier already read it this
            // way (`handle_fiber_abort_signal`, `handle_fiber_abort_signal_jit`).
            //
            // The borrowed-argument retains are consumed above on this path too,
            // and the block's own `DecrefValueRegion` for each then loads the
            // `nil` the take stamped and no-ops: one release either way.
            if bits.is_empty() {
                let (_, result) =
                    self.fiber.signal.take().expect(
                        "VM bug: a tail signal handler that answers SIG_OK sets fiber.signal",
                    );
                self.fiber.stack.push(result);
                return None;
            }
            return Some(bits);
        }

        if let Some((id, default)) = func.as_parameter() {
            let Some(value) = self.call_parameter(id, default, args.len()) else {
                return Some(SIG_ERROR);
            };
            self.fiber.signal = Some((SIG_OK, value));
            return Some(SIG_OK);
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count (skip if compiler verified)
            if !checked && !self.check_arity(&closure.template.arity(), args.len()) {
                // check_arity sets fiber.signal to (SIG_ERROR, ...)
                return Some(SIG_ERROR);
            }

            // Build the pending call, which counts the call as `try_jit_call`
            // counts a non-tail one, so a function reached only in tail position
            // still becomes hot, and carries its compiled code where the cache
            // holds some (docs/impl/jit.md). An environment built for bytecode
            // mints each env value a region of its own (`env_value_region`), so
            // the env cells of two tail calls through this site never share one
            // (region-tail-env-commingle.lisp).
            //
            // A tail call MOVES its args: the caller's reference transfers to
            // the callee (its dead post-tailcall decref never fires), and the
            // owned-param callee releases it. So NO caller incref here.
            //
            // A SPLICED tail call has no such reference to move: its arguments
            // came out of an args array the calling convention owns, whose
            // counted edge the caller reclaims once this returns. So the callee
            // mints one of its own per parameter, as in call position.
            //
            // A late mint is the borrowed argument's reference, made now that the
            // callee is known to release it. It is made before the pending call
            // is built, which releases a moved rest argument as surplus, or
            // resolves that release for a compiled callee
            // (docs/impl/region/handoff.md).
            let late = stash
                .late_mints
                .offered()
                .mint(unsafe { &mut *self.heap_ptr }, &args);
            let Some(tail) = self.pending_tail_call_to(func, closure, args, spliced_args) else {
                late.unmint(unsafe { &mut *self.heap_ptr });
                return Some(SIG_ERROR);
            };

            // Take over the callee closure's release when the compiler flagged it
            // as a per-call local closure whose release is dead past this
            // `TailCall` (`lower_call`'s `defer_callee_release`). The activation
            // discharges this region when it ENDS — the missing decref the frame
            // replacement skipped. The closure must stay alive through the
            // callee's run, so the release is deferred, NOT done here. A
            // program-root callee is never flagged, so its program-lifetime
            // region is never released.
            //
            // Recorded only once the pending call is built, because only
            // then is the frame replacement certain. A refused env — an unknown
            // `&named` key, an odd `&keys` list — raises above with the frame
            // still in place, and the frame's own error exit releases the callee
            // through its release table. A deferral recorded ahead of that
            // refusal releases it a second time where the activation ends
            // (docs/impl/region/owner.md).
            //
            // Recorded on the activation's own dues slot rather than carried on
            // the `TailCallInfo`, because the obligation outlives whoever
            // consumes the pending call: the interpreter trampoline is one such
            // consumer, and a park unwinds it entirely. The current activation is
            // still the CALLER's here, which is the activation the frame
            // replacement hands it to.
            //
            // The arena channel: a letrec body tail-calling a NON-member out of a
            // closure-cycle merged arena carries the arena's static slot
            // (`RegionInfo::cycle_tail_release`), which we resolve through THIS
            // activation's region map — the arena was minted during the letrec
            // setup and its scope-exit `DecrefRegion` is dead past this
            // frame-replacing tail call. The call is built, so the frame IS
            // replaced; the deferred release supplies that dead drop at the
            // recursion's completion. (A native callee never reaches here — it
            // keeps the frame and runs the live scope-exit drop.) See
            // `InstrRef::TailCall::deferred_release_slot`.
            //
            // The two channels are INDEPENDENT, not alternatives. A non-member
            // callee that is itself a per-call local closure strands its own
            // region at the same `TailCall` as the arena's, and each release
            // belongs to a reference the frame separately owns: dropping one for
            // the other strands that reference, and where the callee captures a
            // merge member its own counted edge pins the arena too, so the arena
            // channel alone reclaims nothing (docs/impl/region/letrec.md). They
            // never name the same region — a merge MEMBER callee is absent from
            // `cycle_tail_release`, and `tail_callee_defers_release` refuses every
            // `closure_cycle_members` region.
            let arena =
                deferred_release_slot.and_then(|slot| self.runtime_region_for_release_slot(slot));
            let callee = defer_callee_release
                .then(|| self.tail_callee_release_region(func))
                .flatten();
            for region in arena.into_iter().chain(callee) {
                self.defer_activation_release(region);
            }

            self.pending_tail_call = Some(tail);
            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Some(SIG_OK);
        }

        // Callable collections: struct, array, string, bytes, set — in TAIL
        // position. Routed through `dispatch_collection_call` for the region
        // routing and Rule-5 pass-through retain, then handled with the native-tail
        // trick (see the native branch above): a collection-call pushes NO
        // bytecode frame, so on normal completion we push the result and return
        // `None` rather than replacing the frame. The dispatch loop then
        // CONTINUES into the compiler's post-`TailCall` block, which releases
        // each owned arg with the compiler's exact ownership and retains the
        // result (ReturnValue) — its retain/decref of the result cancel, leaving
        // the pass-through retain as the single caller-consumed reference.
        // Returning `None` here (rather than short-circuiting with
        // `signal = Some((SIG_OK, value)); return SIG_OK`) is what keeps the
        // tail-position call-index from leaking the owned args / the let-bound
        // collection (interpreter) or freeing the returned co-located element
        // under the caller's borrow (JIT).
        if let Some(result) = self.dispatch_collection_call(&func, &args, region_id) {
            match result {
                Ok(value) => {
                    self.fiber.stack.push(value);
                    return None;
                }
                Err((kind, msg)) => {
                    self.set_error(kind, msg);
                    return Some(SIG_ERROR);
                }
            }
        }

        // Cannot call this value
        self.set_error(
            "type-error",
            format!("Cannot call {}", self.show_value(func)),
        );
        Some(SIG_ERROR)
    }
}
