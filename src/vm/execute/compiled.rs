// audited: 2026-11-13
//! A tail callee the JIT compiled: the pending call a tail resolver builds for it, and the trampoline's arm that runs it.
//!
//! docs/impl/jit.md
//! docs/impl/region/owner.md
//! docs/impl/region/relocate.md
//!
//! The compiled callee runs as a native call that opens an activation of its
//! own above the trampoline's, and leaves it in one of five ways: a value, a
//! tail call of its own, a park, an error that parked nothing, or a halt. Each
//! comes back to the trampoline as the `Exit` its loop reads.

use super::Exit;
use crate::hir::region::RuntimeRegion;
use crate::jit::{JitCode, TAIL_CALL_SENTINEL, YIELD_SENTINEL};
use crate::value::arena::{incref_for_escape, region_of, EscapeSite};
use crate::value::fiber::RaiseSite;
use crate::value::{SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_HALT, SIG_OK, SIG_YIELD};
use crate::vm::call::handoff::{fixed_arg_count, param_is_celled, ArgRefs};
use crate::vm::core::{CompiledTail, TailCallInfo, VM};
use std::sync::Arc;

impl VM {
    /// Count a tail call to `closure` for the JIT, and answer the compiled code
    /// the trampoline runs it with: the cached code, while the native stack has
    /// room for it (docs/impl/jit.md). Counting runs no code, so the reserve
    /// does not gate it.
    pub(in crate::vm) fn tail_callee_code(
        &mut self,
        closure: &crate::value::Closure,
    ) -> Option<Arc<JitCode>> {
        let code = self.profile_jit_candidate(closure)?;
        let low = crate::vm::native_stack::below(crate::vm::native_stack::COMPILED_CALL_RESERVE);
        (!low).then_some(code)
    }

    /// The pending tail call that runs `code`, the compiled body of `closure`,
    /// the value `func`, on `args`. The callee starts only once the trampoline
    /// runs it, so each reference it needs is settled here:
    ///
    /// - A MOVED call hands each argument's reference over. A `&` list takes a
    ///   reference of its own to each member, so the moved one is surplus, and
    ///   its region is resolved now for the trampoline to release once the code
    ///   returns (docs/impl/region/relocate.md).
    /// - A SPLICED call's arguments are held by the args array, which the call
    ///   reclaims before the trampoline runs. Each owned parameter's reference
    ///   is minted now, as a call in call position mints it, and every other
    ///   argument, which a `&` list or a cell takes over, is held until the
    ///   code returns (docs/impl/region/mechanism.md).
    pub(in crate::vm) fn compiled_tail_call(
        &mut self,
        code: Arc<JitCode>,
        func: Value,
        closure: &crate::value::Closure,
        args: Vec<Value>,
        spliced_args: bool,
    ) -> TailCallInfo {
        let heap = unsafe { &mut *self.heap_ptr };
        let fixed = fixed_arg_count(closure, &args);
        let releases: Vec<RuntimeRegion> = if spliced_args {
            crate::jit::dispatch::incref_owned_call_args(heap, closure, &args, ArgRefs::Mint);
            let held: Vec<RuntimeRegion> = args
                .iter()
                .enumerate()
                .filter(|&(i, _)| i >= fixed || param_is_celled(closure, i))
                .filter_map(|(_, &arg)| region_of(heap, arg))
                .collect();
            for &region in &held {
                incref_for_escape(heap, Some(region), EscapeSite::CallArgument);
            }
            held
        } else if fixed < args.len() {
            Self::moved_rest_releases(&args[fixed..], &args, heap)
        } else {
            Vec::new()
        };
        let boundary = crate::value::closure::Boundary::of(closure);
        TailCallInfo::compiled(code, func, args, releases, boundary)
    }

    /// Run `tail`, the compiled body of the tail callee `closure_val`, in the
    /// trampoline's activation, and answer the exit the trampoline reads.
    ///
    /// The resolver took every reference the callee owns, so the call mints
    /// none (`ArgRefs::Move`). The references the resolver left for the
    /// trampoline are released once the code returns, however it returns.
    pub(super) fn run_compiled_tail_callee(
        &mut self,
        closure_val: Value,
        tail: CompiledTail,
    ) -> Exit {
        let closure = closure_val
            .as_closure()
            .expect("VM bug: a compiled tail callee is a closure");
        // The resolver's hand-off to the dispatch loop it returned through.
        self.fiber.signal = None;
        let result = self.call_jit(
            tail.code(),
            closure,
            tail.args(),
            closure_val,
            ArgRefs::Move,
        );
        for region in tail.into_releases() {
            self.heap().decref_region(region);
        }
        let bits = self
            .fiber
            .signal
            .map_or(SignalBits::EMPTY, |(bits, _)| bits);
        if bits.intersects(SIG_HALT) {
            return Exit::at(bits, 0);
        }
        if result == YIELD_SENTINEL {
            self.park_dues_with_tail_callee();
            return Exit::at(if bits.is_empty() { SIG_YIELD } else { bits }, 0);
        }
        // A silent callee returns on any signal, and parks nothing: a kill, or a
        // broken proof. It ends the activation it replaced
        // (docs/impl/region/unwind.md).
        if bits.intersects(SIG_ERROR) {
            return Exit::raised(bits, 0, RaiseSite::TailEnded);
        }
        if result == TAIL_CALL_SENTINEL {
            // The callee's own tail call strands its releases on the one-shot
            // for the next activation to open, and none opens: this one runs the
            // next callee too (docs/impl/region/relocate.md).
            for region in std::mem::take(&mut self.pending_tail_deferrals) {
                self.defer_activation_release(region);
            }
            self.fiber.signal = Some((SIG_OK, Value::NIL));
            return Exit::at(SignalBits::EMPTY, 0);
        }
        self.fiber.signal = Some((SIG_OK, result.to_value()));
        Exit::at(SignalBits::EMPTY, 0)
    }

    /// Move what the trampoline's activation owes into the frame the compiled
    /// tail callee parked, the last of the chain, since that activation never
    /// runs again (docs/impl/region/owner.md). The deferred sets join, and the
    /// activation's owner node becomes the frame's, or hands its members to the
    /// frame's node where the frame has one. Where the callee parked no frame,
    /// the dues stay with the activation.
    fn park_dues_with_tail_callee(&mut self) {
        let dues = self.take_activation_dues();
        let heap = unsafe { &mut *self.heap_ptr };
        let Some(SuspendedFrame::Bytecode(frame)) = self
            .fiber
            .suspended
            .as_mut()
            .and_then(|frames| frames.last_mut())
        else {
            *self.activation_dues() = dues;
            return;
        };
        for region in dues.deferred {
            frame.activation_dues.defer(region);
        }
        if let Some(node) = dues.owner_node {
            match frame.activation_dues.owner_node {
                None => frame.activation_dues.owner_node = Some(node),
                Some(own) => {
                    heap.reparent_owned_children(node, own);
                    heap.decref_region_if_present(node);
                }
            }
        }
    }
}
