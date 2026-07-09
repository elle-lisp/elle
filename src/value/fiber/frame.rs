//! Suspension and call-frame types: `BytecodeFrame` (a parked bytecode
//! execution point), `SuspendedFrame` (one step in a fiber's replay chain),
//! and the execution/stack-trace frames `Frame` / `CallFrame`.

use super::FiberHandle;
use crate::value::closure::Closure;
use crate::value::Value;
use std::rc::Rc;

/// A suspended bytecode execution point.
///
/// Captures everything needed to resume bytecode execution: the bytecode,
/// constants pool, closure environment, instruction pointer, and operand
/// stack state. Used for both signal-based suspension (`fiber/signal`) and
/// yield-based suspension (`yield` instruction).
///
/// `stack` always captures the full operand stack at the moment of suspension.
/// For yield suspension, `ip` points past the `Yield` instruction and the
/// resume value needs to be pushed as the result of the `(yield ...)` expression.
/// For instruction-pause suspension (fuel, signal), `ip` points at the paused
/// instruction and the stack is already complete — no extra value is pushed.
/// The `push_resume_value` field encodes which case applies.
#[derive(Debug, Clone)]
pub struct BytecodeFrame {
    /// Code object to resume executing (bytecode + constants + location map +
    /// child protos). The template-derived half of the execution context; see
    /// [`crate::value::Code`].
    pub code: crate::value::Code,
    /// Closure environment (the per-instance captured half of the context).
    pub env: Rc<Vec<Value>>,
    /// Instruction pointer to resume at
    pub ip: usize,
    /// Operand stack state at suspension
    pub stack: Vec<Value>,
    /// Whether to push `current_value` onto the stack before resuming.
    ///
    /// `true` for yield frames and caller frames: the resume value is the
    /// "return value" of the suspended operation (the yield expression result,
    /// or the return value of a call).  `false` for fuel-pause and
    /// signal-pause frames: the instruction at `ip` re-executes from scratch
    /// with the stack exactly as saved — no extra value is injected.
    pub push_resume_value: bool,
    /// This activation's static→physical region remap at the moment of
    /// suspension (docs/regions/semantics.md — every value its own region). A yield
    /// unwinds the Rust call stack and pops each activation's region frame;
    /// without carrying it here, a region allocated before the yield and
    /// `DecrefRegion`'d after resume would resolve in the wrong frame (a leak,
    /// or — on a static-slot collision — a use-after-free). `resume_suspended`
    /// restores this as the activation's region frame before re-entering.
    pub activation_region_map: rustc_hash::FxHashMap<u32, crate::hir::region::RuntimeRegion>,
    /// This activation's owner node at the moment of suspension — MOVED (taken,
    /// never cloned) out of the activation's `Fiber::activation_owner_nodes`
    /// slot by the suspend site, so the node lives in exactly one place: the
    /// live slot or one parked frame (docs/impl/region/owner.md § "Owner nodes"
    /// — "A park moves the node into the suspended frame"). Its members are
    /// `Owned` (RC frozen) with no other release route, so the node must ride
    /// the park to the resumed body's completion, where the trampoline's
    /// clean-break release frees node + members. `resume_suspended` restores it
    /// into the live slot beside `activation_region_map`. `None` for an
    /// activation that never adopted (the node is minted lazily).
    pub activation_owner_node: Option<crate::hir::region::RuntimeRegion>,
    /// The executing-closure register (`Fiber::current_closure`) at the moment this
    /// activation suspended. A yield unwinds the Rust call stack and restores the
    /// live register to the caller's value, so without parking it here a self-edge
    /// resolved after resume would name the wrong closure. `resume_suspended`
    /// re-installs it before re-entering the body. An uncounted borrow that may be
    /// DEAD by resume time — the region solver frees a closure value at its last
    /// use while the activation's `code`/`env` live on as `Rc`s — so it is never
    /// dereferenced on restore; it is live exactly where it is read (`LoadSelf`,
    /// whose self-recursive body keeps its closure region alive through the
    /// recursion). `NIL` for an untracked activation (e.g. a JIT-built frame).
    pub current_closure: Value,
    /// Debug-only snapshot of the uncounted region borrows `activation_region_map`
    /// holds at suspension: `(slot, region, generation)` per mapped region. The
    /// suspended-frame analogue of the cross-fiber param snapshot's `param_borrows`
    /// (docs/impl/region/generations.md § "Two borrow shapes"). The mapped regions
    /// are this activation's own allocations, kept alive by its still-pending
    /// `DecrefRegion`s while the fiber is parked; `resume_suspended` re-checks each
    /// recorded generation before `restore_activation_region_map`, so a region freed
    /// while parked panics at the resume boundary (naming the slot) instead of
    /// corrupting the resumed activation's allocs/decrefs. Filled by
    /// [`BytecodeFrame::suspend`] via `record_region_borrows`; empty in release.
    #[cfg(debug_assertions)]
    pub region_borrows: Vec<(u32, crate::hir::region::RuntimeRegion, u32)>,
}

impl BytecodeFrame {
    /// Build a suspended bytecode frame, snapshotting (debug builds) the uncounted
    /// region borrows its `activation_region_map` holds — the suspended-frame
    /// recorded-generation handle (docs/impl/region/generations.md § "Two borrow
    /// shapes"). Every suspend site goes through this one constructor
    /// ("constructor, not literal"), so the snapshot is taken once, centrally, at the
    /// moment the map is captured. `heap` is read only under `debug_assertions`; the
    /// signature is build-agnostic so suspend sites need no `cfg`.
    #[allow(clippy::too_many_arguments)]
    pub fn suspend(
        code: crate::value::Code,
        env: Rc<Vec<Value>>,
        ip: usize,
        stack: Vec<Value>,
        push_resume_value: bool,
        activation_region_map: rustc_hash::FxHashMap<u32, crate::hir::region::RuntimeRegion>,
        activation_owner_node: Option<crate::hir::region::RuntimeRegion>,
        current_closure: Value,
        heap: &crate::value::fiberheap::FiberHeap,
    ) -> Self {
        #[cfg(debug_assertions)]
        let region_borrows = record_region_borrows(&activation_region_map, heap);
        #[cfg(not(debug_assertions))]
        let _ = heap;
        BytecodeFrame {
            code,
            env,
            ip,
            stack,
            push_resume_value,
            activation_region_map,
            activation_owner_node,
            current_closure,
            #[cfg(debug_assertions)]
            region_borrows,
        }
    }
}

/// Snapshot the uncounted region borrows a suspended bytecode frame's
/// `activation_region_map` holds: for each `(slot, region)`, record `(slot, region,
/// current generation)`. The suspended-frame analogue of `record_param_borrows`
/// (the cross-fiber param snapshot, `src/vm/fiber.rs`); the recorded generation lets
/// `resume_suspended`'s `first_stale_borrow` confirm the region has not been freed
/// since the fiber parked. The region and its generation are read from the SAME
/// `heap`, so the recorded pair and the later check compare within one store. Debug
/// builds only (docs/impl/region/generations.md § "Two borrow shapes").
#[cfg(debug_assertions)]
pub(crate) fn record_region_borrows(
    map: &rustc_hash::FxHashMap<u32, crate::hir::region::RuntimeRegion>,
    heap: &crate::value::fiberheap::FiberHeap,
) -> Vec<(u32, crate::hir::region::RuntimeRegion, u32)> {
    map.iter()
        .map(|(&slot, &r)| (slot, r, heap.generation_raw(r.get())))
        .collect()
}

/// A suspended execution step — either a bytecode frame or a sub-fiber resume.
///
/// The `suspended` Vec on a `Fiber` contains a chain of these, replayed
/// innermost-first by `resume_suspended`.
///
/// - `Bytecode`: resume bytecode execution at a saved instruction pointer.
/// - `FiberResume`: resume a suspended sub-fiber (e.g. a `defer`/`protect`
///   body fiber) with the value flowing through the chain.  This is used when
///   a sub-fiber's I/O signal propagates through its parent: the parent saves
///   a `FiberResume` frame so that on re-entry the I/O result is delivered to
///   the sub-fiber first, and the sub-fiber's final return value then flows
///   into the next frame in the chain (typically the outer `BytecodeFrame`
///   that continues the `defer`/`protect` expansion after `fiber/resume`).
#[derive(Debug, Clone)]
pub enum SuspendedFrame {
    /// Resume bytecode execution from a saved point.
    Bytecode(BytecodeFrame),
    /// Resume a suspended sub-fiber with the incoming value, then continue
    /// to the next frame in the chain with the sub-fiber's return value.
    FiberResume {
        /// Handle to the suspended sub-fiber.
        handle: FiberHandle,
        /// The cached `Value` wrapping `handle` (for child-chain wiring).
        fiber_value: Value,
    },
}

/// A single call frame within a fiber (for execution dispatch).
#[derive(Debug, Clone)]
pub struct Frame {
    /// The closure being executed
    pub closure: Rc<Closure>,
    /// Instruction pointer (byte offset into bytecode)
    pub ip: usize,
    /// Base index in the fiber's operand stack for this frame's temporaries
    pub base: usize,
}

/// Call frame for stack traces (name + ip + frame_base).
/// Separate from Frame because stack traces need human-readable names,
/// while execution dispatch needs closure references.
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub name: Rc<str>,
    pub ip: usize,
    pub frame_base: usize,
    pub location_map: Rc<crate::error::LocationMap>,
}
