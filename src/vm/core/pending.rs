// audited: 2026-10-22
//! The one-shot records a call or a resume leaves on the VM for the driver that completes it.
//!
//! docs/impl/vm.md

use crate::value::{FiberHandle, Value};
use std::rc::Rc;

pub(crate) struct TailCallInfo {
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// The callee CLOSURE this tail call re-enters the activation as. A
    /// frame-replacing tail call keeps the same Rust/activation frame but swaps
    /// which closure is executing (a self-recursive `loop` re-enters as itself; a
    /// tail call to a sibling re-enters as the sibling). `replace_by_tail_call` installs
    /// it as `fiber.current_closure` on the replacement so the executing-closure
    /// register tracks the frame across TCO. `NIL` for a callee with no closure
    /// value (a native/parameter tail target sets no pending tail call, so this is
    /// always a real closure in practice).
    pub closure: Value,
    /// The boundary the callee's signal crosses when the activation ends.
    pub boundary: crate::value::closure::Boundary,
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
