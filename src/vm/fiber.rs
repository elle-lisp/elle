//! Fiber execution: resume, propagate, abort, cancel.
//!
//! All fiber operations follow the same swap protocol:
//! 1. Take child fiber out of its handle
//! 2. Wire parent/child chain (Janet semantics)
//! 3. Swap parent out, child in
//! 4. Execute the child
//! 5. Set provisional status (Dead or Suspended)
//! 6. Extract result
//! 7. Swap back
//! 8. Put child back into its handle
//!
//! Status finalization happens in the caller, not in `with_child_fiber`:
//! - Resume: SIG_ERROR + uncaught by mask → Error (terminal)
//! - Resume: SIG_ERROR + caught by mask → Suspended (resumable)
//! - Abort: inject error + resume, result handled like resume (no stomp)
//! - Cancel: hard kill — set status to Error, drop frames, no resume
//!
//! SIG_TERMINAL signals are uncatchable — they pass through mask checks.

#[cfg(feature = "jit")]
use crate::jit::JitValue;
use crate::value::fiber::FiberStatus;
use crate::value::{
    BytecodeFrame, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_FUEL, SIG_HALT,
    SIG_OK, SIG_SWITCH, SIG_TERMINAL, SIG_YIELD,
};
use std::rc::Rc;

use super::core::VM;

/// Incref the region of a fiber `signal`'s value, if it lives in a region
/// (no-op for `None` and region-0 immediates). The matching decref is the
/// `signal` scan in `find_object_cross_refs`'s Fiber arm, run when the fiber's
/// heap object is freed (cascade-decref) — never an explicit release, since
/// a terminal-result fiber is read (`fiber/value`) but not resumed again.
fn incref_signal_region(
    heap: &mut crate::value::fiberheap::FiberHeap,
    signal: &Option<(SignalBits, Value)>,
) {
    if let Some((_, v)) = signal {
        let r = crate::value::arena::region_of(heap, *v);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::TerminalSignal,
        );
    }
}

/// Release the carrier pass-through retain `dispatch_native_call` applied when a
/// `fiber/resume` primitive returned `(SIG_RESUME, carrier)`.
///
/// `prim_fiber_resume` returns the fiber *argument* (the carrier) as its signal
/// value, so `dispatch_native_call` — which cannot tell a signal payload from a
/// real result — increfs `region_of(carrier)` (the fiber's own region) as the
/// `NativeCallResult` pass-through, expecting the caller's `DecrefValueRegion`
/// to balance it. But the resume handler REPLACES the carrier with the child's
/// actual result before pushing it, so the caller's decref targets the result's
/// region, never the carrier's. The carrier retain is left dangling and the
/// now-:dead fiber's region (dragging its closure + template) leaks
/// (`oracle.lisp`'s `fiber-resume` probe pins this reclaimed).
///
/// Release it exactly when the resumed fiber ran to completion. The child's
/// result is balanced on its own: a fresh result by its alloc reference + the
/// caller's `DecrefValueRegion`; a terminal heap result additionally by the
/// park-retain (`incref_signal_region`) + the free-time signal scan — so no
/// re-target incref of the result is needed.
///
/// Only the completion path releases. A fiber that SUSPENDED (yield / I/O)
/// stays alive and resumable; its carrier retain is the liveness hold the
/// scheduler leans on between pumps, and releasing it would free a live
/// suspended fiber (the protect/scheduler regression that sank the
/// suppress-at-dispatch attempts). So this fires solely on :dead.
fn release_completed_resume_carrier(
    heap: &mut crate::value::fiberheap::FiberHeap,
    fiber_value: Value,
) {
    let r = crate::value::arena::region_of(heap, fiber_value);
    crate::value::arena::decref_region(heap, r);
}

/// A terminal signal is a fiber's *result*: normal return (SIG_OK), error, or
/// halt — read later via `fiber/value`, never resumed. Yield and other
/// suspending signals are transient (the fiber runs again), so their `signal`
/// value is NOT region-pinned. Must agree with the `find_object_cross_refs` Fiber
/// arm so the park-retain and the free-time cascade-decref stay balanced.
pub(crate) fn is_terminal_signal(bits: SignalBits) -> bool {
    bits.is_ok() || bits.contains(SIG_ERROR) || bits.contains(SIG_HALT)
}

/// Release the `SuspendEscape` an io op left on its IoRequest's region when a
/// parked fiber is resumed and that request — held in `fiber.signal` across the
/// park — is replaced by `resume_value`. This reclaims the IoRequest region of a
/// yielding io op — the gauge is `oracle.lisp`'s `io-yield ev/sleep` probe, which
/// dropped 5.5 → 4.5 net objects/op when this landed (the ≈4.5 residual is the
/// general escape-imprecision gap, not this mechanism).
///
/// A yielding io op (`ev/sleep`, `port/read`, …) returns its `IoRequest` with
/// `SIG_IO`, whereupon the suspend adds a
/// [`SuspendEscape`](crate::value::arena::EscapeSite::SuspendEscape) retain so the
/// scheduler can read the request out of `fiber.signal`. The request's own
/// allocation ref is consumed by the scheduler's `fiber/value` read while it
/// submits, so at resume the `SuspendEscape` is the request region's *sole*
/// remaining reference. On resume the io call "returns" `resume_value` (the
/// completion), so the caller's `DecrefValueRegion` targets THAT region, never
/// the request's — orphaning the `SuspendEscape` and leaking the request region,
/// unbounded in a long-running io loop. One decref here, the symmetric
/// counterpart of the suspend-time incref, frees it: the request is dead (the
/// scheduler already consumed it), so its region holds nothing live.
///
/// **Skip when `resume_value` shares the region** — the `Fresh` io ops
/// (`port/read`/`accept`) build their completion buffer *in* the IoRequest's
/// region and hand it back as the resume value, so that region is still live;
/// there the caller's `DecrefValueRegion` on the buffer balances the
/// `SuspendEscape`, and a decref here would free the buffer out from under the
/// caller (a use-after-free).
///
/// Gated on `SIG_IO`. A user `(yield v)` / `(emit …)` value is **body-owned** —
/// its region is released by the fiber body's own `DecrefRegion`, not by a
/// caller's `DecrefValueRegion` — so releasing it here would double-free; only an
/// io op's request is the orphaned, transient native-call result this balances.
/// A no-op for a non-io signal, an immediate / `None` value, or a region-0 value.
pub(crate) fn release_parked_signal(
    heap: &mut crate::value::fiberheap::FiberHeap,
    parked: Option<(SignalBits, Value)>,
    resume_value: Value,
) {
    let Some((bits, value)) = parked else {
        return;
    };
    if !bits.contains(crate::value::SIG_IO) {
        return;
    }
    let region = crate::value::arena::region_of(heap, value);
    if region.is_none() {
        return;
    }
    // The resume value sharing the request's region is the `Fresh`-io-op signature
    // (the completion buffer is built there): that region is still live, so leave
    // it to the caller's `DecrefValueRegion`.
    if crate::value::arena::region_of(heap, resume_value) == region {
        return;
    }
    crate::value::arena::decref_region(heap, region);
}

/// Everything a fiber owns through the ownership forest, TAKEN out of the fiber
/// (its slots emptied) so the release can run after the fiber borrow is dropped
/// (docs/impl/region-model.md § "Owner nodes" — "Fiber teardown frees everything
/// the fiber owns"). Splitting the take from the release keeps heap mutation
/// disjoint from fiber access: the release's cascades can free the fiber's own
/// heap value without invalidating a live borrow.
pub(crate) struct FiberOwned {
    /// Each still-parked `BytecodeFrame`'s activation owner node, in chain order.
    parked_nodes: Vec<crate::hir::region::RuntimeRegion>,
    /// The fiber's own owner node ([`Fiber::fiber_owner_node`]).
    fiber_node: Option<crate::hir::region::RuntimeRegion>,
}

/// The parked activation owner nodes of an abandoned frame chain. The frames'
/// continuations will never run, so the completion release that would have freed
/// each node never fires; the release belongs to whoever abandoned the chain —
/// the discard chokepoint (`VM::discard_suspended_frames`) or the terminal-fiber
/// teardown ([`take_fiber_owned`]). A `FiberResume` frame owns no node (its
/// sub-fiber has its own lifecycle).
pub(crate) fn parked_owner_nodes(
    frames: Vec<SuspendedFrame>,
) -> impl Iterator<Item = crate::hir::region::RuntimeRegion> {
    frames.into_iter().filter_map(|frame| match frame {
        SuspendedFrame::Bytecode(f) => f.activation_owner_node,
        SuspendedFrame::FiberResume { .. } => None,
    })
}

/// Take everything a TERMINAL fiber owns — the parked chain's activation owner
/// nodes and the fiber owner node — emptying the fiber's slots so no other
/// release path can reach them (the move discipline that makes the teardown the
/// sole demise). Pair with [`release_fiber_owned`] once the fiber borrow is
/// dropped. Terminal means the fiber can never be resumed: `:dead` (completion,
/// halt) or a hard kill (cancel; abort of a not-yet-started fiber). An `:error`
/// fiber is NOT terminal — it is resumable (the restarts system replays its
/// re-parked frame) — so an error promotion must never take its owned state.
pub(crate) fn take_fiber_owned(fiber: &mut crate::value::fiber::Fiber) -> FiberOwned {
    let parked_nodes = fiber
        .suspended
        .take()
        .map(|frames| parked_owner_nodes(frames).collect())
        .unwrap_or_default();
    FiberOwned {
        parked_nodes,
        fiber_node: fiber.fiber_owner_node.take(),
    }
}

/// Free everything a terminal fiber owned (the [`take_fiber_owned`] set). When a
/// fiber node exists, each parked node's members are first gathered under it
/// (`reparent_owned_children`) and the emptied node freed, so the teardown is ONE
/// set-drop over the fiber's whole owned set; with no fiber node each parked node
/// subtree-drops directly. One tolerant decref per node: rc 1→0, subtree drop over
/// node + adopted members (interior cycles reclaim with the set), the Shared
/// frontier cascading once from the recorded `outgoing` tables.
pub(crate) fn release_fiber_owned(
    heap: &mut crate::value::fiberheap::FiberHeap,
    owned: FiberOwned,
) {
    let FiberOwned {
        parked_nodes,
        fiber_node,
    } = owned;
    for node in parked_nodes {
        if let Some(fnode) = fiber_node {
            heap.reparent_owned_children(node, fnode);
        }
        heap.decref_region_if_present(node);
    }
    if let Some(fnode) = fiber_node {
        heap.decref_region_if_present(fnode);
    }
}

/// The hard-kill teardown `fiber/cancel` (of a new/parked fiber) and
/// `fiber/abort` (of a not-yet-started one) route through: set the terminal
/// error state, drop the parked chain, and free everything the fiber owned.
/// The take runs under the fiber borrow; the release after it is dropped
/// ([`take_fiber_owned`]). Unlike an ordinary `:error` promotion — which keeps
/// the fiber resumable — a hard kill consumes the chain, so nothing it owned can
/// ever be replayed.
pub(crate) fn kill_fiber(
    heap: &mut crate::value::fiberheap::FiberHeap,
    handle: &FiberHandle,
    error_value: Value,
) {
    let owned = handle.with_mut(|fiber| {
        fiber.status = FiberStatus::Error;
        fiber.signal = Some((SIG_ERROR, error_value));
        take_fiber_owned(fiber)
    });
    release_fiber_owned(heap, owned);
}

mod abort;
mod child;
mod jit;
mod propagate;
mod resume;
mod trampoline;

#[cfg(all(test, debug_assertions))]
mod borrow_tests;

/// Flatten dynamic parameter frames into a single baseline frame, later
/// frames overriding earlier ones. Used when a child fiber inherits its
/// parent's dynamic bindings on first resume.
fn flatten_param_frames(frames: &[Vec<(u32, Value)>]) -> Vec<(u32, Value)> {
    let mut flat: Vec<(u32, Value)> = Vec::new();
    for frame in frames {
        for &(id, val) in frame {
            if let Some(pos) = flat.iter().position(|&(k, _)| k == id) {
                flat[pos].1 = val;
            } else {
                flat.push((id, val));
            }
        }
    }
    flat
}

/// Snapshot the uncounted cross-fiber borrows in a child's inherited baseline
/// parameter frame: for each heap-valued binding, record `(param_id, region,
/// current generation)`. Seeding the baseline takes no reference count, so this
/// records the generation at which each borrowed region is live; the resume and
/// `resolve_parameter` checks later confirm the region has not been freed since.
/// Immediate (non-heap) bindings carry no region and are skipped. Debug-only —
/// the check it feeds runs only under `debug_assertions`
/// (docs/impl/region-generations.md § "Uncounted-borrow check").
///
/// The region and its generation are read from the SAME `heap`
/// (`region_of_ptr`/`generation_raw`), so the recorded pair and the later
/// check compare generations within one store — never across stores, where
/// generations are unrelated numbers.
#[cfg(debug_assertions)]
fn record_param_borrows(
    flat: &[(u32, Value)],
    heap: &crate::value::fiberheap::FiberHeap,
) -> Vec<(u32, crate::hir::region::RuntimeRegion, u32)> {
    flat.iter()
        .filter_map(|&(pid, v)| {
            if !v.is_heap() {
                return None;
            }
            let ptr = v.as_heap_ptr()?;
            let r = crate::hir::region::RuntimeRegion::new(heap.region_of_ptr(ptr))?;
            Some((pid, r, heap.generation_raw(r.get())))
        })
        .collect()
}

/// The first recorded borrow whose region's generation has moved since it was
/// snapshotted — i.e. the region's pages were freed (and possibly recycled)
/// while a fiber still borrowed a value in it. Reads only the generation
/// counter, never the borrowed value's page, so it is sound to call after the
/// region was freed (a re-claimed page would pass `region_of`'s stamp check;
/// the recorded generation catches it). `None` when every borrow is still live.
#[cfg(debug_assertions)]
pub(crate) fn first_stale_borrow(
    borrows: &[(u32, crate::hir::region::RuntimeRegion, u32)],
    heap: &crate::value::fiberheap::FiberHeap,
) -> Option<(u32, crate::hir::region::RuntimeRegion)> {
    borrows
        .iter()
        .find(|&&(_, r, gen)| heap.generation_raw(r.get()) != gen)
        .map(|&(pid, r, _)| (pid, r))
}

impl VM {
    /// Promote a fiber to `:dead` and free everything it owns — the halt-promotion
    /// twin of `with_child_fiber`'s completion arm (docs/impl/region-model.md
    /// § "Owner nodes" — "Fiber teardown frees everything the fiber owns"). Dead is
    /// terminal (`fiber/resume` refuses it), so the parked chain and owner nodes can
    /// never be replayed. An `:error` promotion must NOT come here — an errored
    /// fiber is resumable (the restarts system) and keeps its parked state.
    pub(crate) fn finalize_dead_fiber(&mut self, handle: &FiberHandle) {
        let owned = handle.with_mut(|fiber| {
            fiber.status = FiberStatus::Dead;
            take_fiber_owned(fiber)
        });
        release_fiber_owned(unsafe { &mut *self.heap_ptr }, owned);
    }

    /// Inheritance a trampolined resume must propagate eagerly, while the
    /// calling fiber is still the active one. `do_fiber_resume_single` will
    /// run for the child from the ROOT fiber's context (the trampoline in
    /// `do_fiber_resume`), so anything it derives from `self.fiber` there
    /// would come from the wrong fiber:
    /// - a New child's dynamic parameter baseline (mirrors the inheritance
    ///   in `do_fiber_resume_single`, which skips already-seeded children);
    /// - withheld capabilities (monotonic OR, so `with_child_fiber`'s later
    ///   parent→child OR is harmless on top).
    fn seed_child_inheritance(&mut self, handle: &FiberHandle, fiber_value: Value) {
        let child_is_new = handle.with(|f| f.status == FiberStatus::New);
        if child_is_new && !self.fiber.param_frames.is_empty() {
            let flat = flatten_param_frames(&self.fiber.param_frames);
            #[cfg(debug_assertions)]
            let borrows = record_param_borrows(&flat, self.heap());
            handle.with_mut(|c| {
                c.param_frames = vec![flat];
                #[cfg(debug_assertions)]
                {
                    c.param_borrows = borrows;
                }
            });
        }
        let withheld = self.fiber.withheld;
        handle.with_mut(|f| f.withheld |= withheld);
        // Child-chain wiring: while we are suspended awaiting this child,
        // `fiber/child` of this fiber must report it (recursion parity —
        // with_child_fiber would have set this on us had we recursed).
        self.fiber.child = Some(handle.clone());
        self.fiber.child_value = Some(fiber_value);
    }

    // ── Shared swap protocol ────────────────────────────────────────

    // ── SIG_RESUME: fiber execution ───────────────────────────────

    /// Handle SIG_RESUME from a fiber primitive (Call position).
    ///
    /// At the ROOT (no enclosing fiber): calls `do_fiber_resume`, whose
    /// trampoline drives arbitrarily deep nesting iteratively.
    ///
    /// INSIDE a fiber (current_fiber_handle is Some): recursing into
    /// `do_fiber_resume` here would add Rust stack frames per nesting level
    /// and overflow the host stack at depth. Instead the calling fiber
    /// suspends with its continuation frame, hands the child to the driving
    /// trampoline via `pending_fiber_resume`, and returns SIG_SWITCH. The
    /// trampoline descends iteratively; on the child's completion its unwind
    /// loop resumes our continuation frame with the result pushed.
    pub(super) fn handle_fiber_resume_signal(
        &mut self,
        fiber_value: Value,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_RESUME with non-fiber value");
                self.fiber.stack.push(Value::NIL);
                return None;
            }
        };

        if self.current_fiber_handle.is_some() {
            self.seed_child_inheritance(&handle, fiber_value);
            let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
            let activation_region_map = self
                .fiber
                .activation_region_maps
                .last()
                .cloned()
                .unwrap_or_default();
            // MOVE the caller's owner node into its continuation park — this
            // activation unwinds with the SIG_SWITCH handoff
            // (docs/impl/region-model.md § "Owner nodes").
            let activation_owner_node = self.take_activation_owner_node();
            // The closure that called `fiber/resume` is the one to resume into.
            let current_closure = self.fiber.current_closure;
            let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                code.clone(),
                closure_env.clone(),
                *ip,
                caller_stack,
                true,
                activation_region_map,
                activation_owner_node,
                current_closure,
                self.heap(),
            ));
            self.fiber.suspended = Some(vec![caller_frame]);
            let parent = self
                .current_fiber_handle
                .clone()
                .map(|h| (h, self.current_fiber_value));
            self.pending_fiber_resume = Some(super::core::PendingFiberResume {
                handle,
                fiber_value,
                parent,
            });
            self.fiber.signal = Some((SIG_SWITCH, Value::NIL));
            return Some(SIG_SWITCH);
        }

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.contains(SIG_HALT) {
            self.finalize_dead_fiber(&handle);
        }

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL));
        if caught {
            // Fiber ran to completion (:dead): drop the misdirected carrier
            // pass-through so the now-dead fiber's region is reclaimed. See
            // `release_completed_resume_carrier`. Caught-but-suspended results
            // (a yield the mask covers) leave the fiber resumable, so they keep
            // the carrier retain.
            if result_bits.is_ok() {
                release_completed_resume_carrier(unsafe { &mut *self.heap_ptr }, fiber_value);
            }
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.stack.push(result_value);
            None
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }

            if self.current_fiber_handle.is_none()
                && !result_bits.contains(SIG_ERROR)
                && !result_bits.contains(SIG_HALT)
            {
                self.set_error(
                    "state-error",
                    "fiber/resume: cannot propagate signal (no parent fiber to catch it)",
                );
                self.fiber.stack.push(Value::NIL);
                None
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if result_bits.contains(SIG_ERROR) || result_bits.contains(SIG_HALT) {
                    self.fiber.stack.push(Value::NIL);
                    None
                } else {
                    let fiber_resume_frame = SuspendedFrame::FiberResume {
                        handle: handle.clone(),
                        fiber_value,
                    };
                    let caller_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                    let activation_region_map = self
                        .fiber
                        .activation_region_maps
                        .last()
                        .cloned()
                        .unwrap_or_default();
                    // MOVE the caller's owner node into its continuation park —
                    // this activation unwinds with the child's suspending
                    // signal (docs/impl/region-model.md § "Owner nodes").
                    let activation_owner_node = self.take_activation_owner_node();
                    // Caller activation's remap (the resumed sub-fiber runs
                    // on its own frame stack; this frame continues us).
                    let current_closure = self.fiber.current_closure;
                    let caller_frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                        code.clone(),
                        closure_env.clone(),
                        *ip,
                        caller_stack,
                        true,
                        activation_region_map,
                        activation_owner_node,
                        current_closure,
                        self.heap(),
                    ));
                    self.fiber.suspended = Some(vec![fiber_resume_frame, caller_frame]);
                    Some(result_bits)
                }
            }
        }
    }

    /// Handle SIG_RESUME from a fiber primitive (TailCall position).
    ///
    /// Same trampoline split as the Call-position variant. In tail position
    /// this fiber has NO continuation — its result IS the child's result —
    /// so it suspends with an empty frame list: when the child completes,
    /// `resume_suspended([])` immediately completes this fiber with the
    /// child's value and the unwind continues to our own parent.
    pub(super) fn handle_fiber_resume_signal_tail(&mut self, fiber_value: Value) -> SignalBits {
        let handle = match fiber_value.as_fiber() {
            Some(h) => h.clone(),
            None => {
                self.set_error("internal-error", "SIG_RESUME with non-fiber value");
                return SIG_ERROR;
            }
        };

        if self.current_fiber_handle.is_some() {
            self.seed_child_inheritance(&handle, fiber_value);
            let existing = self.fiber.suspended.take().unwrap_or_default();
            self.fiber.suspended = Some(existing);
            let parent = self
                .current_fiber_handle
                .clone()
                .map(|h| (h, self.current_fiber_value));
            self.pending_fiber_resume = Some(super::core::PendingFiberResume {
                handle,
                fiber_value,
                parent,
            });
            self.fiber.signal = Some((SIG_SWITCH, Value::NIL));
            return SIG_SWITCH;
        }

        let (result_bits, result_value) = self.do_fiber_resume(&handle, fiber_value);
        let mask = handle.with(|fiber| fiber.mask);

        if result_bits.contains(SIG_HALT) {
            self.finalize_dead_fiber(&handle);
        }

        let caught = result_bits.is_ok()
            || (mask.covers(result_bits) && !result_bits.contains(SIG_TERMINAL));
        if caught {
            // :dead — release the misdirected carrier retain (see Call variant).
            if result_bits.is_ok() {
                release_completed_resume_carrier(unsafe { &mut *self.heap_ptr }, fiber_value);
            }
            self.fiber.child = None;
            self.fiber.child_value = None;
            self.fiber.signal = Some((SIG_OK, result_value));
            SIG_OK
        } else {
            if result_bits.contains(SIG_ERROR) {
                handle.with_mut(|f| f.status = FiberStatus::Error);
            }

            if self.current_fiber_handle.is_none()
                && !result_bits.contains(SIG_ERROR)
                && !result_bits.contains(SIG_HALT)
            {
                self.set_error(
                    "state-error",
                    "fiber/resume: cannot propagate signal (no parent fiber to catch it)",
                );
                SIG_ERROR
            } else {
                self.fiber.signal = Some((result_bits, result_value));
                if !result_bits.contains(SIG_ERROR) && !result_bits.contains(SIG_HALT) {
                    let fiber_resume_frame = SuspendedFrame::FiberResume {
                        handle: handle.clone(),
                        fiber_value,
                    };
                    let mut existing = self.fiber.suspended.take().unwrap_or_default();
                    let mut all = vec![fiber_resume_frame];
                    all.append(&mut existing);
                    self.fiber.suspended = Some(all);
                }
                result_bits
            }
        }
    }

    /// Execute a fiber resume with trampoline for nested fiber/resume.
    ///
    /// If the child fiber itself calls `fiber/resume` (setting
    /// `pending_fiber_resume` and returning SIG_SWITCH), we iterate
    /// rather than recursing on the Rust call stack.
    ///
    /// `pub(crate)` (not `pub(super)`): the fiber-lifecycle runtime tests drive a
    /// child fiber through park, resume, and completion with this entry.
    pub(crate) fn do_fiber_resume(
        &mut self,
        child_handle: &FiberHandle,
        child_value: Value,
    ) -> (SignalBits, Value) {
        let (bits, value) = self.do_fiber_resume_single(child_handle, child_value);
        self.finish_fiber_resume(bits, value, child_handle, child_value)
    }

    // ── JIT-context fiber signal handlers ────────────────────────────
    //
    // These mirror the interpreter-level handlers above but return `JitValue`
    // instead of pushing to fiber.stack. Called from jit/dispatch.rs when a
    // primitive returns SIG_RESUME/SIG_PROPAGATE/SIG_ABORT in JIT context.
}
