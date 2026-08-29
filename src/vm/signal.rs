//! Primitive signal dispatch.
//!
//! Routes signal bits returned by NativeFn primitives to the appropriate
//! handler: stack push for SIG_OK, error storage for SIG_ERROR, fiber
//! execution for SIG_RESUME/SIG_PROPAGATE/SIG_ABORT, VM state reads
//! for SIG_QUERY.

use crate::signals::dispatch::{classify, SignalAction};
use crate::value::{BytecodeFrame, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_OK};
use std::rc::Rc;

use super::core::VM;

mod query;

mod modules;

mod config;

impl VM {
    /// Handle signal bits returned by a primitive in a Call position.
    ///
    /// Returns `None` to continue the dispatch loop, or `Some(bits)` to
    /// return from the dispatch loop (for yields/signals).
    pub(super) fn handle_primitive_signal(
        &mut self,
        bits: SignalBits,
        value: Value,
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        if !bits.is_empty() {
            etrace!(
                self,
                crate::config::trace_bits::SIGNAL,
                "signal",
                "bits={} value_type={}",
                bits,
                value.type_name()
            );
        }

        match classify(bits, &value) {
            SignalAction::Ok => {
                self.fiber.stack.push(value);
                None
            }
            SignalAction::Resume => self.handle_fiber_resume_signal(value, code, closure_env, ip),
            SignalAction::Propagate => self.handle_fiber_propagate_signal(value),
            SignalAction::Abort => self.handle_fiber_abort_signal(value, code, closure_env, ip),
            SignalAction::Query => {
                // A query answered outside `dispatch_native_call` (no compiler
                // result slot): build a boundary ctx so the answer is born on a
                // fresh region of the VM's heap, freed value-based by the
                // consumer.
                let mut ctx =
                    crate::primitives::ctx::Alloc::boundary(unsafe { &mut *self.heap_ptr });
                let (sig, result) = self.dispatch_query(&mut ctx, value);
                if sig.intersects(SIG_ERROR) {
                    self.fiber.signal = Some((sig, result));
                    self.fiber.stack.push(Value::NIL);
                } else {
                    self.fiber.stack.push(result);
                }
                None
            }
            SignalAction::Error => {
                self.fiber.signal = Some((bits, value));
                self.fiber.stack.push(Value::NIL);
                None
            }
            SignalAction::Halt => {
                self.fiber.signal = Some((bits, value));
                Some(bits)
            }
            SignalAction::Suspend => {
                // The yielded value escapes into `fiber.signal`, where the
                // scheduler reads it (e.g. an IoRequest handed to io/submit).
                // Retain its region: the escape is the "+1 incremented
                // elsewhere" of the prediction-free model. The caller's
                // `DecrefValueRegion` at the yielding call's decref_point (which
                // fires against the resume value when the fiber continues)
                // would otherwise drop this region's only reference and free
                // the value out from under the scheduler.
                let heap = unsafe { &mut *self.heap_ptr };
                let r = crate::value::arena::region_of(heap, value);
                crate::value::arena::incref_for_escape(
                    heap,
                    r,
                    crate::value::arena::EscapeSite::SuspendEscape,
                );
                // This primitive never returns, so the frame's continuation —
                // the code after the Call, including the call's own result
                // release — is funded by the delivery instead of by a `Return`
                // mint (docs/impl/region/owner.md § "A delivery into a replayed
                // frame carries one owning reference").
                self.fiber.resume_value_unfunded = true;
                let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
                let activation_region_map = self
                    .fiber
                    .activation_region_maps
                    .last()
                    .cloned()
                    .unwrap_or_default();
                // MOVE the activation's owner node into the frame (its slot is
                // likewise still on top) so it rides the park to the resumed
                // body's completion (docs/impl/region/owner.md § "Owner nodes").
                let activation_owner_node = self.take_activation_owner_node();
                // Suspending primitive: this activation's remap is still on top
                // (the wrapping `saving_stack` pops it after we return), and the
                // current closure is this activation's — park it for the resume.
                let current_closure = self.fiber.current_closure;
                let frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
                    code.clone(),
                    closure_env.clone(),
                    *ip,
                    saved_stack,
                    true,
                    activation_region_map,
                    activation_owner_node,
                    current_closure,
                    self.heap(),
                ));
                self.fiber.signal = Some((bits, value));
                self.fiber.suspended = Some(vec![frame]);
                Some(bits)
            }
        }
    }

    /// Handle signal bits returned by a primitive in a TailCall position.
    ///
    /// Always returns SignalBits (tail calls always return from the dispatch loop).
    pub(super) fn handle_primitive_signal_tail(
        &mut self,
        bits: SignalBits,
        value: Value,
    ) -> SignalBits {
        if !bits.is_empty() {
            etrace!(
                self,
                crate::config::trace_bits::SIGNAL,
                "signal",
                "tail bits={} value_type={} action={:?}",
                bits,
                value.type_name(),
                classify(bits, &value)
            );
        }
        match classify(bits, &value) {
            SignalAction::Ok => {
                self.fiber.signal = Some((SIG_OK, value));
                SIG_OK
            }
            SignalAction::Resume => self.handle_fiber_resume_signal_tail(value),
            SignalAction::Propagate => self.handle_fiber_propagate_signal_tail(value),
            SignalAction::Abort => self.handle_fiber_abort_signal_tail(value),
            SignalAction::Query => {
                // Tail-position mirror of the Call-position Query arm: build a
                // boundary ctx so the answer is born on a fresh region of the
                // VM's heap.
                let mut ctx =
                    crate::primitives::ctx::Alloc::boundary(unsafe { &mut *self.heap_ptr });
                let (sig, result) = self.dispatch_query(&mut ctx, value);
                self.fiber.signal = Some((sig, result));
                sig
            }
            SignalAction::Suspend => {
                // Tail-position mirror of the Call-position Suspend retain
                // (see `handle_primitive_signal`): the yielded value escapes
                // into `fiber.signal` for the scheduler, so retain its region
                // to survive the caller's `DecrefValueRegion` at the yielding
                // tail call's decref_point.
                let heap = unsafe { &mut *self.heap_ptr };
                let r = crate::value::arena::region_of(heap, value);
                crate::value::arena::incref_for_escape(
                    heap,
                    r,
                    crate::value::arena::EscapeSite::SuspendEscape,
                );
                // Tail-position mirror of the Call-position park classification.
                // A tail suspend builds no frame of its own — the driver it
                // unwinds to parks one — so the obligation rides the fiber until
                // the delivery. The frame that driver parks resumes into the
                // post-`TailCall` block, whose result release the missing `Return`
                // mint would have funded.
                self.fiber.resume_value_unfunded = true;
                self.fiber.signal = Some((bits, value));
                bits
            }
            SignalAction::Error | SignalAction::Halt => {
                self.fiber.signal = Some((bits, value));
                bits
            }
        }
    }

    // ── Capability denial ─────────────────────────────────────────────

    /// Handle capability denial in Call position.
    ///
    /// The fiber tried to call a primitive whose signal bits overlap with
    /// the fiber's `withheld` capabilities. Instead of running the primitive,
    /// emit a signal with the blocked bits and a denial payload struct.
    pub(super) fn handle_capability_denial(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
        blocked: SignalBits,
        args: &[Value],
        code: &crate::value::Code,
        closure_env: &Rc<Vec<Value>>,
        ip: &mut usize,
    ) -> Option<SignalBits> {
        let payload = {
            let mut ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *self.heap_ptr });
            Self::build_denial_payload(&mut ctx, def, blocked, args)
        };

        // The denial payload escapes into `fiber.signal`, read later via
        // `fiber/value` after control has left this fiber. Like the
        // `SignalAction::Suspend` path (see `handle_primitive_signal`), retain
        // its region: the resumer's `DecrefValueRegion` at the denied call's
        // decref_point would otherwise drop this freshly-built struct's only
        // reference (rc=1, the owning activation scope) and free it out from
        // under `fiber/value`.
        let heap = unsafe { &mut *self.heap_ptr };
        let r = crate::value::arena::region_of(heap, payload);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::SuspendEscape,
        );
        // The denied primitive never runs, let alone returns, so the mediating
        // parent's resume value takes the place of its result — and the frame's
        // continuation releases that result (see the `SignalAction::Suspend` arm).
        self.fiber.resume_value_unfunded = true;
        // Which is why the payload's own birth reference reaches no consumer past
        // the park: the continuation's release names the resume value, not this
        // struct. Record it, because the install that displaces the park owes that
        // reference and only this classifier can tell a denial from an `(emit …)`
        // under the same withheld bits (docs/impl/region/owner.md § "Park/unpark
        // symmetry").
        self.fiber.denial_payload = Some(payload);

        // Save the stack and build a suspended frame (same as suspending signals)
        let saved_stack: Vec<Value> = self.fiber.stack.drain(..).collect();
        let activation_region_map = self
            .fiber
            .activation_region_maps
            .last()
            .cloned()
            .unwrap_or_default();
        // MOVE the activation's owner node into the frame (see the Suspend arm).
        let activation_owner_node = self.take_activation_owner_node();
        // Capability denial suspends the current activation; its remap is still on
        // top (the wrapping `saving_stack` pops it after return), and its closure
        // is the current one — park it for the resume.
        let current_closure = self.fiber.current_closure;
        let frame = SuspendedFrame::Bytecode(BytecodeFrame::suspend(
            code.clone(),
            closure_env.clone(),
            *ip,
            saved_stack,
            true,
            activation_region_map,
            activation_owner_node,
            current_closure,
            self.heap(),
        ));
        self.fiber.signal = Some((blocked, payload));
        self.fiber.suspended = Some(vec![frame]);
        Some(blocked)
    }

    /// Handle capability denial in TailCall position.
    pub(super) fn handle_capability_denial_tail(
        &mut self,
        def: &'static crate::primitives::def::PrimitiveDef,
        blocked: SignalBits,
        args: &[Value],
    ) -> SignalBits {
        let payload = {
            let mut ctx = crate::primitives::ctx::Alloc::new(unsafe { &mut *self.heap_ptr });
            Self::build_denial_payload(&mut ctx, def, blocked, args)
        };
        // Retain the escaping payload region, mirroring the tail-position
        // `SignalAction::Suspend` path (see `handle_primitive_signal_tail`):
        // the payload is read later via `fiber/value`, so it must survive the
        // resumer's `DecrefValueRegion` at this tail call's decref_point.
        let heap = unsafe { &mut *self.heap_ptr };
        let r = crate::value::arena::region_of(heap, payload);
        crate::value::arena::incref_for_escape(
            heap,
            r,
            crate::value::arena::EscapeSite::SuspendEscape,
        );
        // Tail-position mirror of the Call-position denial park (see
        // `handle_capability_denial`), delivery obligation and left-over payload
        // reference alike.
        self.fiber.resume_value_unfunded = true;
        self.fiber.denial_payload = Some(payload);
        self.fiber.signal = Some((blocked, payload));
        blocked
    }

    /// Build the denial payload struct.
    ///
    /// Returns `{:error :capability-denied :denied <keyword-set>
    ///           :primitive <name> :func <native-fn> :args <array>}`.
    /// Build the denial payload through `ctx` so every heap field (`:denied`
    /// set, `:primitive` string, `:args` array) and the struct itself are born
    /// in the call's own region. Shared with the JIT denial path
    /// (`crate::jit::calls::jit_capability_denial`), which lacks the interpreter's
    /// pre-call capability gate and reuses this builder verbatim.
    pub(crate) fn build_denial_payload(
        ctx: &mut crate::primitives::ctx::Alloc,
        def: &'static crate::primitives::def::PrimitiveDef,
        blocked: SignalBits,
        args: &[Value],
    ) -> Value {
        use crate::value::heap::TableKey;
        use std::collections::BTreeMap;

        let registry = crate::signals::registry::global_registry().lock().unwrap();
        let denied_keywords = registry.bits_to_keywords(blocked);

        let denied = ctx.set(denied_keywords.into_iter().collect());
        let primitive = ctx.string(def.name);
        let args_array = ctx.array(args.to_vec());

        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("error".into()),
            Value::keyword("capability-denied"),
        );
        fields.insert(TableKey::Keyword("denied".into()), denied);
        fields.insert(TableKey::Keyword("primitive".into()), primitive);
        fields.insert(TableKey::Keyword("func".into()), Value::native_fn(def));
        fields.insert(TableKey::Keyword("args".into()), args_array);

        ctx.struct_from(fields)
    }
}

impl VM {}

impl VM {}

#[cfg(test)]
mod tests;
