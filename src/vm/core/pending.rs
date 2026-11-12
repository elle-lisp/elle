// audited: 2026-11-11
//! The one-shot records a call or a resume leaves on the VM for the driver that completes it.
//!
//! docs/impl/vm.md

use crate::value::{FiberHandle, Value};
use std::rc::Rc;

/// A tail call to a closure, handed from the resolver that built it to the
/// trampoline that runs its callee in place of the activation's body. Built and
/// read through its methods.
pub(crate) struct TailCallInfo {
    code: crate::value::Code,
    env: Rc<Vec<Value>>,
    /// The callee CLOSURE this tail call re-enters the activation as. A
    /// frame-replacing tail call keeps the same Rust/activation frame but swaps
    /// which closure is executing (a self-recursive `loop` re-enters as itself; a
    /// tail call to a sibling re-enters as the sibling). `run_tail_callee` installs
    /// it as `fiber.current_closure` on the replacement so the executing-closure
    /// register tracks the frame across TCO.
    closure: Value,
    /// The boundary the callee's signal crosses when the activation ends.
    boundary: crate::value::closure::Boundary,
}

impl TailCallInfo {
    /// A tail call that runs `closure`'s body, `code`, as bytecode with the
    /// environment `env` the resolver built.
    pub(crate) fn interpreted(
        code: crate::value::Code,
        env: Rc<Vec<Value>>,
        closure: Value,
        boundary: crate::value::closure::Boundary,
    ) -> Self {
        TailCallInfo {
            code,
            env,
            closure,
            boundary,
        }
    }

    /// The callee closure the activation runs as once the call replaces it.
    pub(crate) fn closure(&self) -> Value {
        self.closure
    }

    /// The body the activation runs once the call replaces it: the callee's
    /// code and environment.
    pub(crate) fn body(&self) -> (crate::value::Code, Rc<Vec<Value>>) {
        (self.code.clone(), self.env.clone())
    }

    /// Take the boundary the callee's signal crosses, for the activation's
    /// [`TailBoundary`](crate::value::fiber::TailBoundary) or a sentinel
    /// consumer's own boundary to add.
    pub(crate) fn take_boundary(&mut self) -> crate::value::closure::Boundary {
        std::mem::take(&mut self.boundary)
    }
}

/// A non-tail call to an interpreted closure, handed from `call_inner` to
/// `run_dispatch`, which pauses the caller in the fiber and runs the callee on
/// the same dispatch loop (docs/impl/vm.md).
pub(crate) struct PendingCall {
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// The callee closure value, which becomes the body's executing-closure
    /// register.
    pub closure: Value,
    /// The offset of the call instruction in the caller.
    pub call_ip: usize,
    pub site: crate::value::fiber::CallSite,
}

/// Pending fiber resume for the trampoline.
///
/// Set by `handle_fiber_resume_signal` when it wants to switch fibers
/// without recursing. Consumed by the trampoline in `do_fiber_resume`.
pub(crate) struct PendingFiberResume {
    pub handle: FiberHandle,
    pub fiber_value: Value,
    /// The TRUE parent of the pending child — the fiber whose code called
    /// `fiber/resume`. The trampoline descends from the ROOT context (the
    /// requesting fiber is swapped out by then), so `with_child_fiber`'s
    /// default parent wiring (the currently active fiber) would record the
    /// wrong parent. Carried here and installed via the VM's
    /// `trampoline_parent_override` for the descent.
    pub parent: Option<(FiberHandle, Option<Value>)>,
}
