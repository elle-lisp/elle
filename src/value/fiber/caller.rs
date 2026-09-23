// audited: 2026-09-23
//! A caller paused in its fiber while its interpreted callee runs on the same
//! dispatch loop, and what completing the call needs.
//!
//! docs/impl/vm.md

use super::SignalBits;
use crate::value::Value;
use std::rc::Rc;

/// What completing a non-tail closure call needs to know about its callee.
///
/// Read off the callee when the call is made, because the callee's closure
/// value may be freed at its last use while its body is still running.
#[derive(Debug, Clone, Copy)]
pub struct CallSite {
    /// The callee's squelch mask. A suspending signal it names becomes a
    /// `signal-violation` error at this call.
    pub squelch_mask: SignalBits,
    /// The callee was declared `silence`d, so any signal leaving it is a
    /// programmer error that aborts the process.
    pub silent: bool,
    /// The callee's name, for the silence diagnostic.
    pub name: Option<&'static str>,
}

/// An interpreted callee's activation, running on its caller's dispatch loop.
#[derive(Debug)]
pub struct Activation {
    /// The body being run. A tail call inside the activation replaces it.
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// The squelch masks of every tail call that replaced this activation's
    /// body, OR'd together.
    pub tail_squelch: SignalBits,
    /// The call that entered this activation.
    pub call: CallSite,
    /// How many region-remap frames the fiber held before this activation
    /// pushed its own. Debug builds check that the body left exactly that one
    /// frame above them.
    #[cfg(debug_assertions)]
    pub entry_depth: usize,
}

/// A caller activation waiting in [`Fiber::callers`](super::Fiber::callers)
/// for its callee to finish.
#[derive(Debug)]
pub struct PausedCaller {
    /// The paused activation, or `None` for the activation `run_dispatch` was
    /// entered with, whose code and environment its own caller holds.
    pub activation: Option<Activation>,
    /// Where the caller continues: the instruction after the call.
    pub resume_ip: usize,
    /// The call instruction itself, which an error leaving the callee names as
    /// the caller's location.
    pub call_ip: usize,
    /// The caller's operand stack, locals included.
    pub stack: Vec<Value>,
    /// The caller's executing-closure register.
    pub closure: Value,
}
