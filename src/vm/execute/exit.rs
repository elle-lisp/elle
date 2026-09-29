// audited: 2026-09-28
//! How an activation ends: the dispatch loop's `Exit`, and the `ExecResult` a
//! closed activation hands to whoever parks or completes it.
//!
//! docs/impl/vm.md

use crate::value::fiber::{ActivationDues, RaiseSite};
use crate::value::{SignalBits, Value};
use std::rc::Rc;

/// How one activation's dispatch loop exited: the signal, the ip it stopped
/// at, and, for an error, where the raise happened (docs/impl/vm.md § "The
/// error exit"). The site rides out to the fiber boundary, which records what a
/// restart delivers into.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Exit {
    pub bits: SignalBits,
    pub ip: usize,
    pub site: RaiseSite,
}

impl Exit {
    /// An exit that is not a raise, or a raise whose call left its result
    /// position empty: a callee's error, a tail call's, a spliced call's.
    pub(crate) fn at(bits: SignalBits, ip: usize) -> Self {
        Exit {
            bits,
            ip,
            site: RaiseSite::Call,
        }
    }

    /// A raise at `site`.
    pub(crate) fn raised(bits: SignalBits, ip: usize, site: RaiseSite) -> Self {
        Exit { bits, ip, site }
    }
}

/// How an activation ended, as `execute_bytecode_saving_stack` and
/// `execute_bytecode_from_ip` answer it.
///
/// Contains the signal, the IP, the raise site, the active code object and env
/// at exit, and the inner operand stack at a signal exit.
///
/// When a tail call occurs before a signal, the active context differs from
/// the original closure — callers that create `SuspendedFrame`s must use
/// these fields, not the original closure's bytecode/constants.
///
/// `stack` is what a park without a frame of its own is built from. A fuel
/// pause re-executes the instruction at `ip`, whose arguments are still on the
/// stack, so the stack must be restored exactly as it was. An error park holds
/// the stack with the raising call's result position empty. `SIG_YIELD` is
/// exempt — `handle_emit` drains the stack into `fiber.suspended` before
/// returning, so `fiber.suspended` is already populated and the `stack` field
/// here is unused for that signal.
pub(crate) struct ExecResult {
    pub bits: SignalBits,
    pub ip: usize,
    /// Where an error exit raised, which says what a restart of the park built
    /// from this result delivers into. `Call` for every other exit.
    pub site: RaiseSite,
    /// The active code object at exit (may differ from the input if a tail call
    /// occurred before the signal). The template-derived half of the context.
    pub code: crate::value::Code,
    pub env: Rc<Vec<Value>>,
    /// The inner operand stack at a signal exit; empty on a clean return.
    pub stack: Vec<Value>,
    /// This activation's static→physical region remap, captured by
    /// `close_activation` just before it pops the frame on a suspending exit.
    /// Callers that build a `SuspendedFrame::Bytecode` from the callee's
    /// returned context (the inner/fuel-pause frame) attach this so the remap
    /// survives the yield. Default (empty) for `execute_bytecode_from_ip`,
    /// whose caller manages frames itself.
    pub activation_region_map: rustc_hash::FxHashMap<u32, crate::hir::region::MappedRegion>,
    /// What this activation owed, TAKEN by `close_activation` beside
    /// `activation_region_map` on a non-OK exit — the channel that carries the
    /// record out of the already-popped activation to the caller that builds
    /// its park (`BytecodeFrame::activation_dues`): the fiber body's pause in
    /// `do_fiber_first_resume`, the interrupted callee's inner frame in
    /// `complete_call`. A suspend handler that parked the frame itself (the
    /// yield path) already took the record, so this reads default there — the
    /// move discipline holds. Always default for `execute_bytecode_from_ip`
    /// (`replay_suspended` manages the slot directly).
    pub activation_dues: ActivationDues,
    /// The executing-closure register (`fiber.current_closure`) at the moment the
    /// activation ended — the callee's value, possibly re-installed by tail calls
    /// in this activation. A caller building a `SuspendedFrame` from this returned
    /// context parks it so the self-identity survives the yield. `NIL` for an
    /// untracked activation.
    pub current_closure: Value,
}

impl ExecResult {
    /// The result of an activation whose dispatch loop ended at `exit`. The
    /// region remap and the dues start empty; `close_activation` moves them in
    /// where the activation parks.
    pub(super) fn ended(
        exit: Exit,
        code: crate::value::Code,
        env: Rc<Vec<Value>>,
        stack: Vec<Value>,
        current_closure: Value,
    ) -> Self {
        ExecResult {
            bits: exit.bits,
            ip: exit.ip,
            site: exit.site,
            code,
            env,
            stack,
            activation_region_map: rustc_hash::FxHashMap::default(),
            activation_dues: ActivationDues::default(),
            current_closure,
        }
    }
}
