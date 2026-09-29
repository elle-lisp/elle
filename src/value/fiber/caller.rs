// audited: 2026-09-29
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
    /// The callee's signal is silent, declared or inferred, so any signal
    /// leaving it breaks that claim, and the call aborts the process with a
    /// diagnostic.
    pub silent: bool,
    /// The callee's name, for the silence diagnostic.
    pub name: Option<&'static str>,
}

/// How many `parameterize` frames a fiber held at an entry: a call, a host
/// that runs code, or a squelch boundary. Code that stops running pops none of
/// the frames it pushed, so whatever abandons it truncates the fiber's frames
/// to the depth recorded at its entry ([`super::Fiber::unwind_params`]). Read
/// only off a fiber ([`super::Fiber::param_depth`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamDepth(pub(super) usize);

/// The squelch masks of the tail calls that replaced an activation's body,
/// OR'd together, and the parameter depth at those calls. A body pops its own
/// `parameterize` frames before its tail call, so the depth is where a
/// violation the masks raise truncates to.
#[derive(Debug, Clone, Copy)]
pub struct TailSquelch {
    pub mask: SignalBits,
    pub depth: ParamDepth,
}

impl TailSquelch {
    /// An activation no tail call has replaced yet, entered at `depth`.
    pub fn none(depth: ParamDepth) -> Self {
        TailSquelch {
            mask: SignalBits::EMPTY,
            depth,
        }
    }

    /// A tail call whose callee carries `mask` replaced the body at `depth`.
    pub fn add(&mut self, mask: SignalBits, depth: ParamDepth) {
        self.mask |= mask;
        self.depth = depth;
    }
}

/// An interpreted callee's activation, running on its caller's dispatch loop.
#[derive(Debug)]
pub struct Activation {
    /// The body being run. A tail call inside the activation replaces it.
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// What the tail calls that replaced this activation's body enforce.
    pub tail_squelch: TailSquelch,
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
    /// How many parameter frames the fiber held at the call. A callee that
    /// leaves by an error is abandoned, and the `parameterize` frames it pushed
    /// leave with it (docs/signals/primitives.md § "Where a restart lands").
    pub param_depth: ParamDepth,
    /// The caller's executing-closure register.
    pub closure: Value,
}
