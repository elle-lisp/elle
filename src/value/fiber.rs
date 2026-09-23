// audited: 2026-09-23
//! Fiber types for the Elle runtime.
//!
//! A fiber is an independent execution context: it owns its operand stack,
//! call frames, and signal state. The VM dispatches into the current fiber;
//! suspended fibers are stored as heap values.

use crate::value::closure::Closure;
use crate::value::Value;
use smallvec::SmallVec;
use std::rc::Rc;

// The fiber's cohesive item groups live in submodules; re-exported here so
// every `crate::value::fiber::<Item>` path resolves unchanged.
mod caller;
mod frame;
mod handle;
mod status;
pub use caller::{Activation, CallSite, PausedCaller};
pub use frame::*;
pub use handle::*;
pub use status::*;

mod delivery;
pub use delivery::Delivery;

mod dues;
pub use dues::ActivationDues;

mod parked;
pub use parked::{ParkedDues, ParkedState};

mod signalbits;
pub use signalbits::SignalBits;

// Signal constants are canonically defined in `crate::signals` (the semantic
// owner). Re-exported here so existing `use crate::value::fiber::SIG_*`
// imports continue to work.
pub use crate::signals::{
    SIG_ABORT, SIG_DEBUG, SIG_ERROR, SIG_EXEC, SIG_FFI, SIG_FS, SIG_FUEL, SIG_HALT, SIG_IO, SIG_OK,
    SIG_PROPAGATE, SIG_QUERY, SIG_RESUME, SIG_SWITCH, SIG_TERMINAL, SIG_WAIT, SIG_YIELD,
};

/// The fiber: an independent execution context.
///
/// Holds all per-execution state:
/// operand stack, call frames, exception handlers.
/// The VM retains only shared state (modules, JIT cache, FFI, docs, heap).
///
/// The heap lives on the VM, not on individual fibers. All fibers share
/// the VM's single heap; isolation is per-region.
pub struct Fiber {
    /// Operand stack (temporaries). SmallVec avoids heap allocation for
    /// fibers with fewer than 256 stack entries.
    pub stack: SmallVec<[Value; 256]>,
    /// The caller activations waiting for an interpreted callee, innermost
    /// last. A non-tail call pauses its caller here and runs the callee on the
    /// same dispatch loop, so call depth costs memory rather than native stack
    /// (docs/impl/vm.md § "Non-tail calls"). Empty whenever the fiber is
    /// parked: a suspend moves every paused caller into `suspended`.
    pub callers: Vec<PausedCaller>,
    /// Current status
    pub status: FiberStatus,
    /// Signal mask: which of this fiber's signals are caught by its parent.
    /// Set at creation time by the parent. Immutable after creation.
    pub mask: SignalBits,
    /// Parent fiber (weak to avoid Rc cycles)
    pub parent: Option<WeakFiberHandle>,
    /// Cached Value for the parent fiber. Set during resume chain
    /// wiring. Avoids re-allocating a HeapObject on every `fiber/parent` call.
    pub parent_value: Option<Value>,
    /// Most recently resumed child (for stack traces and resumption routing)
    pub child: Option<FiberHandle>,
    /// Cached Value for the child fiber. Set during resume chain
    /// wiring. Avoids re-allocating a HeapObject on every `fiber/child` call.
    pub child_value: Option<Value>,
    /// The closure this fiber was created from
    pub closure: Rc<Closure>,
    /// The closure VALUE this fiber was created from — the heap value `closure`
    /// was cloned out of — so the fiber's first resume can install it as the
    /// body's executing-closure register (a self-recursive fiber body resolves
    /// its self-reference to it, via `pending_entry_closure`). Its region is a
    /// COUNTED cross-region edge of the fiber (`find_object_cross_refs`'s Fiber
    /// arm): the fiber keeps it alive for its whole life, because a `squelch`/
    /// `attune` wrapper's value lives in a region distinct from the template/env
    /// region — reachable only through this field. The runtime `current_closure`
    /// register that reads it stays an uncounted transient borrow OF this counted
    /// anchor. `NIL` for a fiber whose closure never executes as a body (the root
    /// fiber's dummy, the native-iterator no-op).
    pub closure_value: Value,
    /// Parameter binding frames. Each `parameterize` pushes a frame;
    /// exiting pops it. Lookup walks frames from top to bottom.
    pub param_frames: Vec<Vec<(u32, Value)>>,
    /// True once an inherited parameter BASELINE was installed as
    /// `param_frames[0]` (creation-time snapshot in `prim_fiber_new`, or the
    /// first-resume fallback). The seed retains each heap entry's region and
    /// records a `fiber → value` content edge; the Fiber content-scan arm
    /// visits the baseline exactly when this is set, so the fiber's free
    /// cascade is the symmetric release (docs/impl/region/park.md § "A
    /// child's inherited parameter baseline is a counted holder"). The
    /// fiber's own later `parameterize` frames are not covered — their values
    /// belong to the parked activation.
    pub param_baseline_seeded: bool,
    /// Recorded `(param_id, region, generation)` for the heap values in this
    /// fiber's *inherited baseline* parameter frame. The seed retains each
    /// entry's region (`EscapeSite::ParamBaseline`), so the region outliving
    /// the fiber is an invariant the count upholds; the recorded generation
    /// lets the resume and `resolve_parameter` checks PROVE it (debug
    /// builds), turning a missing or displaced retain into a panic at the
    /// borrow instead of a stale read. Populated only under
    /// `debug_assertions`; empty otherwise (docs/impl/region/generations.md
    /// § "Uncounted-borrow check").
    pub param_borrows: Vec<(u32, crate::hir::region::RuntimeRegion, u32)>,
    /// Signal value from this fiber. Canonical location for both
    /// signal payloads and normal return values.
    /// - On signal: (bits, payload) before suspending
    /// - On normal return: (SIG_OK, return_value) before completing
    pub signal: Option<(SignalBits, Value)>,
    /// The `SIG_ERROR` payload this fiber parked, paired with the source
    /// location of the form that raised it.
    ///
    /// A mask that absorbs the error stops it travelling, so `VM::absorbs`
    /// moves the live record (`VM::error_loc`) here rather than discarding it;
    /// `fiber/propagate` reads it back when it re-raises this fiber's parked
    /// signal, which is how the location survives the `defer` and scheduler
    /// catch-then-re-raise chains (docs/impl/vm.md § "Where a reported error's
    /// location comes from").
    ///
    /// The payload is carried so the reader can tell that the location still
    /// describes the error being re-raised — representation identity, as in
    /// the delivery ledger, never structural equality. A fiber that goes on to
    /// park a different error (an injected abort, a second raise that recorded
    /// no location) therefore lends its old location to nothing.
    ///
    /// Like the ledger's records, the payload is an UNCOUNTED marker: it is
    /// only ever compared bit-wise, never dereferenced, so it takes no retain
    /// and the Fiber content scan records no edge for it. The counted edge for
    /// the same value is `signal`'s. Kept beside the ledger rather than in it:
    /// this is a diagnostics record with a life of its own (it survives the
    /// resume take, waiting for a matching re-raise), not a funding fact.
    pub error_loc: Option<(Value, crate::error::SourceLoc)>,
    /// The delivery ledger: how the current park's delivery references are
    /// funded — the raise-minted payload, the bodyless (denial) payload, and
    /// whether the resume value owes a mint. Written where a park is built,
    /// consumed where the park ends, through a method-only surface
    /// ([`Delivery`]; docs/impl/region/park.md § "A park names its funding in
    /// the delivery ledger").
    pub delivery: Delivery,
    /// Suspended execution frames. Set when the fiber suspends; consumed
    /// when it resumes.
    ///
    /// - Signal suspension (`fiber/signal`): single frame, empty stack
    /// - Yield suspension (`yield`): chain of frames from yielder to
    ///   fiber boundary, each with its operand stack captured
    ///
    /// On resume, frames are replayed from innermost (index 0) to
    /// outermost (last index).
    pub suspended: Option<Vec<SuspendedFrame>>,

    /// Per-activation region-slot remap (docs/impl/region/model.md — every value its
    /// own region). Each entry maps a static bytecode region id (a per-
    /// function "slot") to a fresh physical region id minted for *this*
    /// activation. The stack mirrors the call stack: the top is the current
    /// activation's frame; a closure call pushes a fresh frame, a normal
    /// return pops it, a tail call reuses it. Always non-empty (a base frame
    /// covers the top level). Carried on the fiber so it survives yields.
    pub activation_region_maps: Vec<rustc_hash::FxHashMap<u32, crate::hir::region::MappedRegion>>,

    /// The per-activation DUES slots, parallel to `activation_region_maps` (one
    /// entry per activation frame; pushed/popped only through
    /// `VM::push_activation_region_map` / `restore_activation_region_map` /
    /// `pop_activation_region_map`, which keep the two stacks in lockstep). An
    /// entry holds what the activation owes the region system when it ends: its
    /// pages-less owner-node region — the forest root `AdoptIntoActivation`
    /// adopts members into (docs/impl/region/owner.md § "Owner nodes — an
    /// activation as a forest root") — and the releases it took over from
    /// frame-replacing tail calls. Discharged at the activation's normal
    /// completion (`VM::release_activation_dues`); a suspend MOVES the whole
    /// record into the parked frame ([`BytecodeFrame::activation_dues`]) and the
    /// resume restores it, so both reach that completion across any number of
    /// parks (docs/impl/region/owner.md § "A deferred tail-call release has the
    /// node's life").
    pub activation_dues: Vec<ActivationDues>,

    /// The FIBER's own owner node — the pages-less forest root for a region whose
    /// owner is the fiber itself, outliving every single activation
    /// (docs/impl/region/owner.md § "Owner nodes" — "The fiber owner node").
    /// Fiber state, so it rides parks and fiber swaps structurally — nothing
    /// moves it, unlike the per-activation slots above. Minted lazily; `None`
    /// for a fiber that owns nothing. Freed only at the fiber's terminal
    /// transitions (`take_fiber_owned` / `release_fiber_owned`,
    /// `src/vm/fiber.rs`) — never while the fiber is resumable.
    pub fiber_owner_node: Option<crate::hir::region::RuntimeRegion>,

    /// The closure whose body is currently executing in this fiber — an
    /// **uncounted borrow**, a pure runtime register, not a heap object; it is
    /// the self-identity a self-reference resolves to. An activation can outlive
    /// its closure's heap value (the solver frees the value at its last use while
    /// the body's `code`/`env` live on as `Rc`s), so the register may hold a dead
    /// value for a body that never reads it; it is live exactly where it is read
    /// (`LoadSelf` — a self-recursive body's closure region outlives the
    /// recursion, docs/impl/selfrec.md), and no other site dereferences it.
    /// Snapshotted/restored like `activation_region_maps`: a nested call parks
    /// the caller's in its `PausedCaller` or saves it around a re-entry, a
    /// tail call re-installs it on the frame replacement (`trampoline_loop`), and
    /// a yield parks it in the `BytecodeFrame` and restores it on resume — so it
    /// is per-activation and rides fiber swaps with the fiber, never a VM-global
    /// read across a switch. `NIL` when no closure is executing (the top-level
    /// body) or when an entrant left it untracked.
    pub current_closure: Value,

    // --- Execution state migrated from VM ---
    /// The non-tail closure calls in progress, on every tier. A call past
    /// `RuntimeConfig::max_depth` halts with `:stack-overflow`.
    pub call_depth: usize,
    /// Call stack for stack traces (name + ip + frame_base)
    pub call_stack: Vec<CallFrame>,
    /// Instruction budget. `None` = unlimited (default). `Some(n)` = `n` units
    /// remaining. Decremented at backward jumps and call instructions. When it
    /// reaches zero the VM emits `SIG_FUEL`, pausing the fiber. Refuel via
    /// `fiber/set-fuel` then call `fiber/resume` to continue.
    pub fuel: Option<u32>,
    /// Withheld capabilities. Bits set here prevent the fiber from silently
    /// performing the corresponding operations. When a primitive's signal bits
    /// overlap with `withheld & CAP_MASK`, the primitive is blocked and a
    /// denial signal is emitted instead. Default: empty (full access).
    /// Transitive: `child.withheld = parent.withheld | deny_bits`.
    pub withheld: SignalBits,
    /// Native iterator state for trait-based :iter fibers.
    /// When set, fiber/resume pulls the next value from here instead of
    /// executing bytecode. `None` = normal bytecode fiber.
    pub native_iter: Option<NativeIter>,
}

/// A Rust-side iterator that feeds values to a fiber.
/// Each resume pops the next element; exhaustion kills the fiber.
pub struct NativeIter {
    pub elements: Vec<Value>,
    pub cursor: usize,
}

/// A minimal no-op closure for a fiber that executes no bytecode. Its body is
/// the instance's placeholder code object, never executed — a native-iterator
/// fiber short-circuits in the VM's resume path, and the root fiber's execution
/// context is top-level bytecode rather than a closure.
pub(crate) fn noop_closure(heap: &mut crate::value::fiberheap::FiberHeap) -> Rc<Closure> {
    Rc::new(Closure::new(
        heap.placeholder_template(),
        // The no-op closure captures nothing — an empty env slice needs no
        // region, so build it directly.
        crate::value::region_slice::RegionSlice::empty(),
        SignalBits::EMPTY,
    ))
}

impl Fiber {
    /// Create a new fiber from a closure with the given signal mask.
    pub fn new(closure: Rc<Closure>, mask: SignalBits) -> Self {
        Fiber {
            stack: SmallVec::new(),
            callers: Vec::new(),
            status: FiberStatus::New,
            mask,
            parent: None,
            parent_value: None,
            child: None,
            child_value: None,
            closure,
            closure_value: Value::NIL,
            param_frames: Vec::new(),
            param_baseline_seeded: false,
            param_borrows: Vec::new(),
            signal: None,
            error_loc: None,
            delivery: Delivery::new(),
            suspended: None,
            activation_region_maps: vec![rustc_hash::FxHashMap::default()],
            activation_dues: vec![ActivationDues::default()],
            fiber_owner_node: None,
            current_closure: Value::NIL,
            call_depth: 0,
            call_stack: Vec::new(),
            fuel: None,
            withheld: SignalBits::EMPTY,
            native_iter: None,
        }
    }

    /// Create a native iterator fiber from a Vec of elements.
    ///
    /// Each `fiber/resume` call returns the next element. When all
    /// elements are exhausted, the fiber dies. No bytecode is executed.
    pub fn native_iter(
        heap: &mut crate::value::fiberheap::FiberHeap,
        elements: Vec<Value>,
        mask: SignalBits,
    ) -> Self {
        let closure = noop_closure(heap);
        Fiber {
            stack: SmallVec::new(),
            callers: Vec::new(),
            status: FiberStatus::Paused,
            mask,
            parent: None,
            parent_value: None,
            child: None,
            child_value: None,
            closure,
            closure_value: Value::NIL,
            param_frames: Vec::new(),
            param_baseline_seeded: false,
            param_borrows: Vec::new(),
            signal: None,
            error_loc: None,
            delivery: Delivery::new(),
            suspended: None,
            activation_region_maps: vec![rustc_hash::FxHashMap::default()],
            activation_dues: vec![ActivationDues::default()],
            fiber_owner_node: None,
            current_closure: Value::NIL,
            call_depth: 0,
            call_stack: Vec::new(),
            fuel: None,
            withheld: SignalBits::EMPTY,
            native_iter: Some(NativeIter {
                elements,
                cursor: 0,
            }),
        }
    }

    /// Set an error signal on this fiber, the error value born in the
    /// caller-supplied `region` (Rule 3; docs/impl/region/ctx.md).
    ///
    /// The `Fiber` owns no heap, so the region is minted by the caller: a VM
    /// caller via `VM::set_error` (a fresh
    /// `result_region`); an env-builder caller from its own heap handle. The
    /// error escapes as the fiber's signal payload and is freed value-based by
    /// the consumer's `DecrefValueRegion`.
    #[inline]
    pub fn set_error_in(
        &mut self,
        heap: &mut crate::value::fiberheap::FiberHeap,
        kind: &str,
        msg: impl Into<String>,
        region: crate::hir::region::RuntimeRegion,
    ) {
        self.signal = Some((
            SIG_ERROR,
            crate::value::error_val_in(heap, kind, msg, region),
        ));
    }
}

impl std::fmt::Debug for Fiber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<fiber:{} frames={} stack={}>",
            self.status.as_str(),
            self.callers.len(),
            self.stack.len()
        )
    }
}

/// A fiber value on `heap`, in its own region, with `status` — and the handle
/// that can change it.
///
/// Built rather than run, because what a test using this asks a fiber is only
/// its status: a fiber that reached `:dead` by running and one stamped `:dead`
/// here answer that question identically. The alternative is standing up a VM
/// and a scheduler to reach one state.
#[cfg(test)]
pub fn test_fiber_in_region(
    heap: &mut crate::value::fiberheap::FiberHeap,
    status: FiberStatus,
) -> (Value, FiberHandle) {
    let handle = FiberHandle::new(Fiber::new(noop_closure(heap), SignalBits::EMPTY));
    handle.with_mut(|f| f.status = status);
    let region = heap.new_runtime_region();
    let value = heap.alloc_in_region(
        crate::value::heap::HeapObject::Fiber {
            handle: handle.clone(),
            traits: Value::NIL,
        },
        region,
    );
    (value, handle)
}

#[cfg(test)]
mod tests;
