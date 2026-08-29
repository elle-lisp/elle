//! VM::tail_call_inner — shared TailCall/TailCallArrayMut dispatch.

use super::*;

impl VM {
    /// Resolve this tail call's borrowed-argument retains to physical regions
    /// and CONSUME the local each one is stashed in, without releasing anything
    /// yet (docs/impl/region/mechanism.md § "What the fall-through owes, a
    /// signal exit owes too").
    ///
    /// Stamping the local `nil` is what makes the release count once: a frame
    /// that leaves by a signal can still be REPLAYED — a suspending signal parks
    /// the continuation at the post-`TailCall` ip, and an `:error` fiber is
    /// resumable for the restarts system — so the very `DecrefValueRegion` this
    /// stands in for is reached a second time, where it now loads an immediate
    /// and no-ops. The stash belongs to the block alone: it is written once
    /// before the call and read once after it, for this release and nothing
    /// else.
    ///
    /// Resolving and releasing are split so the release runs AFTER the signal
    /// handler: the handler may install the very value as the signal's payload
    /// (a fiber carrier hands over its own fiber argument) or swap the live
    /// fiber out from under the frame whose locals name it.
    ///
    /// `spare` is the value a SUSPENDING exit parks, and it is left standing: that
    /// exit's continuation is parked at the post-`TailCall` ip, so the release the
    /// retain answers to still runs on the resume, and the retain is the one
    /// reference the park owes the body for what it yields (owner.md § "A fiber
    /// body owns one reference of every value it yields"). Its slot keeps its
    /// value, so the replay releases it rather than no-opping on a stamp. `None`
    /// everywhere else, where nothing reaches the block again.
    fn take_borrowed_arg_retains(
        &mut self,
        slots: &[u16],
        spare: Option<Value>,
    ) -> Vec<crate::hir::region::RuntimeRegion> {
        if slots.is_empty() {
            return Vec::new();
        }
        let frame_base = self.current_frame_base();
        let heap = unsafe { &mut *self.heap_ptr };
        let spared = spare.and_then(|v| crate::value::arena::region_of(heap, v));
        let mut regions = Vec::with_capacity(slots.len());
        for &slot in slots {
            let Some(cell) = self.fiber.stack.get_mut(frame_base + slot as usize) else {
                continue;
            };
            let value = *cell;
            let region = crate::value::arena::result_region_of(heap, value);
            if spared.is_some() && region == spared {
                continue;
            }
            *cell = Value::NIL;
            regions.extend(region);
        }
        regions
    }

    /// Mint the DELIVERY reference of a payload this tail call is RAISING, where
    /// the payload is one of the call's own arguments — the shape a dynamic
    /// `emit` takes, its non-literal first argument making the raise an ordinary
    /// native call (docs/impl/region/mechanism.md § "What the fall-through owes,
    /// a signal exit owes too").
    ///
    /// The catcher's read of the signal consumes exactly one reference, and every
    /// reference this frame holds answers to the frame's own release routes: the
    /// borrowed-argument retain is consumed by the exit (and no-oped at a
    /// restart's replay by its nil stamp), an owned argument's release sits in the
    /// abandoned block for the walk or that same replay to run. So the delivery is
    /// minted here, exactly as `handle_emit` mints it on the literal path — and
    /// recorded with it (`Fiber::emit_delivery`), so the abandoned-frame walk and
    /// the parked frame's discharge stop exempting the payload's region and
    /// reclaim the frame's own reference to a payload it allocated.
    ///
    /// A payload the native BUILT — a fresh error struct — is nobody's argument
    /// and funds the delivery with its birth reference, so the identity test is
    /// what keeps this off every ordinary native raise.
    fn mint_raised_argument_delivery(&mut self, args: &[Value], payload: Value) {
        if !args.iter().any(|a| a.bit_identical(payload)) {
            return;
        }
        let heap = unsafe { &mut *self.heap_ptr };
        // An immediate payload crosses no region, so there is nothing to fund and
        // nothing for the record to say stands.
        let Some(region) = crate::value::arena::region_of(heap, payload) else {
            return;
        };
        crate::value::arena::incref_for_escape(
            heap,
            Some(region),
            crate::value::arena::EscapeSite::EmitEscape,
        );
        self.fiber.emit_delivery = Some(payload);
    }

    /// Release the regions [`Self::take_borrowed_arg_retains`] resolved. Split
    /// from the resolution so the signal handler runs between the two — see that
    /// method for why.
    fn run_borrowed_arg_retains(&mut self, regions: Vec<crate::hir::region::RuntimeRegion>) {
        if regions.is_empty() {
            return;
        }
        let heap = unsafe { &mut *self.heap_ptr };
        for region in regions {
            if crate::config::get().has_trace("rc") {
                eprintln!(
                    "[trace:rc] borrowed-arg retain released({region}) rc={} \
                     (tail-call signal exit)",
                    heap.region_rc(region)
                );
            }
            heap.decref_region(region);
        }
    }

    /// Shared TailCall/TailCallArrayMut logic after argument extraction.
    ///
    /// Dispatches native functions via tail signal handler, sets up pending
    /// tail call for closures with environment building.
    ///
    /// When `checked` is true, the compiler verified arity at compile time
    /// and the runtime skips the arity check for primitives and closures.
    ///
    /// `borrowed_arg_slots` names the frame locals holding this call's
    /// borrowed-argument retains. That retain has exactly one consumer per path:
    /// a frame-replacing closure callee's owned-param release, or the
    /// post-`TailCall` fall-through block, which runs on one outcome only — the
    /// native's normal completion. A SIGNAL exit reaches neither, so it consumes
    /// the retain here instead — except at a SUSPEND, which parks the
    /// continuation at the post-`TailCall` ip, so the block does run on resume and
    /// the retain naming the parked payload is the reference that park owes the
    /// body (docs/impl/region/mechanism.md § "What the fall-through owes, a signal
    /// exit owes too"). A terminal `:error` consumes them like any other exit and
    /// mints the payload's delivery instead
    /// ([`Self::mint_raised_argument_delivery`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn tail_call_inner(
        &mut self,
        func: Value,
        args: Vec<Value>,
        checked: bool,
        region_id: StaticRegion,
        defer_callee_release: bool,
        deferred_release_slot: Option<StaticRegion>,
        borrowed_arg_slots: &[u16],
    ) -> Option<SignalBits> {
        if let Some(def) = func.as_native_def() {
            let blocked = def
                .signal
                .bits
                .intersection(self.fiber.withheld)
                .intersection(crate::signals::CAP_MASK);
            if !blocked.is_empty() {
                // The denial never runs the native at all, so the fall-through
                // block is abandoned exactly as it is on any other signal exit.
                // Its park spares nothing: the payload is a struct the denial
                // built, which names no argument of this call.
                let owed = self.take_borrowed_arg_retains(borrowed_arg_slots, None);
                let bits = self.handle_capability_denial_tail(def, blocked, &args);
                self.run_borrowed_arg_retains(owed);
                return Some(bits);
            }
            if !checked && !def.arity.matches(args.len()) {
                let owed = self.take_borrowed_arg_retains(borrowed_arg_slots, None);
                self.set_error(
                    "arity-error",
                    format!(
                        "{}: expected {} argument(s), got {}",
                        def.name,
                        def.arity,
                        args.len()
                    ),
                );
                self.run_borrowed_arg_retains(owed);
                return Some(SIG_ERROR);
            }
            // Fresh per-execution result region + pass-through retain, shared
            // with the Call-position path and the JIT. (A yielding native's
            // escape retain is handled inside `handle_primitive_signal_tail`'s
            // Suspend arm; a fresh result lives in this call's `alloc_region` and
            // is skipped by `dispatch_native_call`, so the two never double-count.)
            let (bits, value) = self.dispatch_native_call(def, &args, region_id);
            // Native-tail arg release, done the CORRECT way: by NOT
            // replacing the frame for a normally-completing native.
            //
            // The problem: a value whose last use is a native tail-call argument
            // has its owning `DecrefValueRegion` emitted by the compiler AFTER the
            // `TailCall` (the dead post-`TailCall` block: store result, release
            // each owned arg, retain result, `Return`). A frame-replacing tail
            // call skips that block, so the arg leaks
            // (region-native-tail-move.lisp: `(length (%pair 1 nil))`,
            // `(& xs) (length xs)`).
            //
            // A native pushes NO bytecode frame, so a native tail call need not
            // replace the frame at all. On normal completion, push the result and
            // return `None` — exactly the non-tail `handle_primitive_signal` `Ok`
            // path — so the dispatch loop CONTINUES into that post-`TailCall`
            // block and runs the compiler's own owned-arg `DecrefValueRegion`s.
            // This releases each arg with the compiler's EXACT per-arg ownership
            // (a value with a live decref / stored into the result / aliased is
            // NOT over-freed — region-closure-struct.lisp, box.lisp), which a
            // runtime "release every arg" heuristic cannot achieve. Result
            // accounting stays balanced by TWO retains on the result region: the
            // `dispatch_native_call` pass-through retain, consumed by the caller's
            // `DecrefValueRegion`, and the post-`TailCall` block's ReturnValue
            // `IncrefValueRegion` (`lower_call`, the tail mirror of `lower_return`)
            // that keeps the returned value alive for the caller's binding. The
            // ReturnValue retain precedes the owned-arg releases, so a result
            // borrowed from an arg's region survives that arg's cascade-free
            // (region-native-tail-return-uaf.lisp; omitting it freed the result
            // under the caller's borrow).
            //
            // `bits.is_empty()` is exactly `SignalAction::Ok` (see `classify`). A
            // non-OK native carries its value as a SIGNAL that may embed an arg
            // (a yielding `port/write`/`port/flush` hands the scheduler an
            // `IoRequest` embedding the port — a Rule-5 suspend-escape): keep the
            // tail suspend/abort/error path, which retains the yielded value and
            // releases the embedded arg on resume/abort. NOT replacing the
            // frame here would run the dead arg releases and free a port the
            // scheduler still reads (the unmasked escape UAF).
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
            // resume replays it. So the retain naming the value this park delivers
            // is spared — it is the one reference the body owes for what it yields,
            // released by that replayed block or, for a fiber abandoned while
            // suspended, by the discard discharge that stands in for it. Only that
            // one: a park delivers ONE payload and the discharge releases ONE
            // reference of it, so a retain on any other region has no stand-in.
            let action = crate::signals::dispatch::classify(bits, &value);
            let spare =
                matches!(action, crate::signals::dispatch::SignalAction::Suspend).then_some(value);
            let owed = self.take_borrowed_arg_retains(borrowed_arg_slots, spare);
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
            let bits = self.handle_primitive_signal_tail(bits, value);
            self.run_borrowed_arg_retains(owed);
            // A fiber CARRIER (`fiber/resume`/`fiber/abort`/`fiber/propagate`/
            // `fiber/refuse`)
            // leaves the primitive as a signal because it is a request: drive
            // this child fiber and report what happened. Where this fiber's own
            // mask ABSORBS the child's outcome the request is answered here, so
            // the value is the call's result and the frame never left — it takes
            // the fall-through, exactly as a native that completed normally does
            // (docs/impl/region/mechanism.md § "A carrier that comes back with a
            // result never left the frame"). The post-`TailCall` block then runs
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
            if !args.is_empty() {
                self.set_error(
                    "arity-error",
                    format!("parameter call: expected 0 arguments, got {}", args.len()),
                );
                return Some(SIG_ERROR);
            }
            let value = self.resolve_parameter(id, default);
            // Pass-through retain, mirror of the Call-position param branch: a
            // resolve is never a fresh allocation, so always hand the caller one
            // owning reference for its `DecrefValueRegion` to consume.
            // `incref_for_escape(None, …)` no-ops an immediate.
            let heap = unsafe { &mut *self.heap_ptr };
            let result_region = crate::value::arena::region_of(heap, value);
            crate::value::arena::incref_for_escape(
                heap,
                result_region,
                crate::value::arena::EscapeSite::ParameterResolve,
            );
            self.fiber.signal = Some((SIG_OK, value));
            return Some(SIG_OK);
        }

        if let Some(closure) = func.as_closure() {
            // Validate argument count (skip if compiler verified)
            if !checked && !self.check_arity(&closure.template.arity, args.len()) {
                // check_arity sets fiber.signal to (SIG_ERROR, ...)
                return Some(SIG_ERROR);
            }

            // Take over the callee closure's release when the compiler flagged it
            // as a per-call local closure whose release is dead past this
            // `TailCall` (`lower_call`'s `defer_callee_release`). The new
            // activation releases this
            // region when it completes — the missing decref the frame replacement
            // skipped. Recorded BEFORE `populate_env` (which copies the closure's
            // env uncounted): the closure must stay alive through the callee's
            // run, so the release is deferred to the trampoline-loop break, NOT
            // done here. A program-root callee is never flagged, so its
            // program-lifetime region is never released. See `TailCallInfo`.
            //
            // The arena channel: a letrec body tail-calling a NON-member out of a
            // closure-cycle merged arena carries the arena's static slot
            // (`RegionInfo::cycle_tail_release`), which we resolve through THIS
            // activation's region map — the arena was minted during the letrec
            // setup and its scope-exit `DecrefRegion` is dead past this
            // frame-replacing tail call. We reached the closure arm, so the frame
            // IS replaced; the deferred release supplies that dead drop at the
            // recursion's completion. (A native callee never reaches here — it
            // keeps the frame and runs the live scope-exit drop.) See
            // `LirInstr::TailCall::deferred_release_slot`.
            //
            // The two channels are INDEPENDENT, not alternatives. A non-member
            // callee that is itself a per-call local closure strands its own
            // region at the same `TailCall` as the arena's, and each release
            // belongs to a reference the frame separately owns: dropping one for
            // the other strands that reference, and where the callee captures a
            // merge member its own counted edge pins the arena too, so the arena
            // channel alone reclaims nothing (docs/impl/region/letrec.md § "The
            // arena channel and the callee channel are independent"). They never
            // name the same region — a merge MEMBER callee is absent from
            // `cycle_tail_release`, and `tail_callee_defers_release` refuses every
            // `closure_cycle_members` region.
            let deferred = crate::vm::core::DeferredReleases {
                arena: deferred_release_slot
                    .and_then(|slot| self.runtime_region_for_release_slot(slot)),
                callee: defer_callee_release
                    .then(|| self.tail_callee_release_region(func))
                    .flatten(),
            };

            // Build proper environment using cached vector. Each env value mints
            // its own fresh region inside `populate_env` (see `env_value_region`),
            // so the static slot is no longer used as a physical region — only
            // its identity as a static slot is consulted. Each tail call through
            // this site mints its own env-cell regions, so they never commingle
            // (region-tail-env-commingle.lisp pins this).
            // A tail call MOVES its args: the caller's reference transfers to
            // the callee (its dead post-tailcall decref never fires), and the
            // owned-param callee releases it. So NO caller incref here —
            // `own_params = false`. (The non-tail path, `build_closure_env`,
            // passes `true`.)
            if !Self::populate_env(
                &mut self.tail_call_env_cache,
                unsafe { &mut *self.heap_ptr },
                &mut self.fiber,
                closure,
                &args,
                false,
            ) {
                return Some(SIG_ERROR);
            }
            let new_env_rc = Rc::new(self.tail_call_env_cache.clone());

            // Store the tail call information (Rc clones, not data copies)
            self.pending_tail_call = Some(crate::vm::core::TailCallInfo {
                code: closure.template.code(),
                env: new_env_rc,
                closure: func,
                squelch_mask: closure.squelch_mask,
                deferred,
            });

            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Some(SIG_OK);
        }

        // Callable collections: struct, array, set — in TAIL position. Routed
        // through `dispatch_collection_call` for the per-execution region +
        // Rule-5 pass-through retain, then handled with the native-tail
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
        eprintln!(
            "[DEBUG tailcall] Cannot call: tag={:#x} payload={:#x} type={} on_fiber_heap={}",
            func.tag,
            func.payload,
            func.type_name(),
            self.heap().value_in_region_store(func)
        );
        self.set_error("type-error", format!("Cannot call {:?}", func));
        Some(SIG_ERROR)
    }
}
