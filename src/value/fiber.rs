//! Fiber types for the Elle runtime.
//!
//! A fiber is an independent execution context: it owns its operand stack,
//! call frames, and signal state. The VM dispatches into the current fiber;
//! suspended fibers are stored as heap values.

use crate::value::closure::Closure;
use crate::value::Value;
use smallvec::SmallVec;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

// ---------------------------------------------------------------------------
// FiberHandle / WeakFiberHandle
// ---------------------------------------------------------------------------

/// A handle to a fiber that supports take/put semantics.
///
/// Wraps `Rc<RefCell<Option<Fiber>>>`. The `Option` makes "fiber is currently
/// executing on the VM" representable as `None` — no dummy fiber needed.
///
/// - `take()` extracts the fiber (sets slot to None)
/// - `put()` returns the fiber (sets slot to Some)
/// - `with()`/`with_mut()` borrow in-place for read/write
/// - `try_with()` returns None if the fiber is taken or already borrowed
#[derive(Clone)]
pub struct FiberHandle(Rc<RefCell<Option<Fiber>>>);

impl FiberHandle {
    /// Create a new handle wrapping a fiber.
    pub fn new(fiber: Fiber) -> Self {
        FiberHandle(Rc::new(RefCell::new(Some(fiber))))
    }

    /// Take the fiber out of the handle. Panics if already taken.
    pub fn take(&self) -> Fiber {
        self.0
            .borrow_mut()
            .take()
            .expect("FiberHandle::take: fiber already taken (currently executing on VM)")
    }

    /// Stable identity for this fiber (Rc pointer address).
    /// Used by the WASM backend to key per-fiber suspension frame storage.
    pub fn id(&self) -> usize {
        Rc::as_ptr(&self.0) as usize
    }

    /// Put a fiber back into the handle. Panics if slot is occupied.
    pub fn put(&self, fiber: Fiber) {
        let mut slot = self.0.borrow_mut();
        assert!(
            slot.is_none(),
            "FiberHandle::put: slot already occupied (fiber not taken)"
        );
        *slot = Some(fiber);
    }

    /// Borrow the fiber immutably. Panics if taken.
    pub fn with<R>(&self, f: impl FnOnce(&Fiber) -> R) -> R {
        let borrow = self.0.borrow();
        let fiber = borrow
            .as_ref()
            .expect("FiberHandle::with: fiber is taken (currently executing on VM)");
        f(fiber)
    }

    /// Borrow the fiber mutably. Panics if taken.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Fiber) -> R) -> R {
        let mut borrow = self.0.borrow_mut();
        let fiber = borrow
            .as_mut()
            .expect("FiberHandle::with_mut: fiber is taken (currently executing on VM)");
        f(fiber)
    }

    /// Try to borrow the fiber immutably. Returns None if taken or already
    /// mutably borrowed (used by Debug/Display where panicking is wrong).
    pub fn try_with<R>(&self, f: impl FnOnce(&Fiber) -> R) -> Option<R> {
        let borrow = self.0.try_borrow().ok()?;
        let fiber = borrow.as_ref()?;
        Some(f(fiber))
    }

    /// Create a weak reference to this handle.
    pub fn downgrade(&self) -> WeakFiberHandle {
        WeakFiberHandle(Rc::downgrade(&self.0))
    }
}

impl std::fmt::Debug for FiberHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.try_with(|fib| fib.status.as_str().to_string()) {
            Some(status) => write!(f, "<fiber-handle:{}>", status),
            None => write!(f, "<fiber-handle:taken>"),
        }
    }
}

/// A weak reference to a FiberHandle, used for parent back-pointers
/// to avoid Rc cycles.
#[derive(Clone)]
pub struct WeakFiberHandle(Weak<RefCell<Option<Fiber>>>);

impl WeakFiberHandle {
    /// Attempt to upgrade to a strong FiberHandle. Returns None if the
    /// fiber has been dropped.
    pub fn upgrade(&self) -> Option<FiberHandle> {
        self.0.upgrade().map(FiberHandle)
    }
}

impl std::fmt::Debug for WeakFiberHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<weak-fiber-handle>")
    }
}

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
    /// live slot or one parked frame (docs/impl/region-model.md § "Owner nodes"
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
    /// (docs/impl/region-generations.md § "Two borrow shapes"). The mapped regions
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
    /// recorded-generation handle (docs/impl/region-generations.md § "Two borrow
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
/// builds only (docs/impl/region-generations.md § "Two borrow shapes").
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

mod signalbits;
pub use signalbits::SignalBits;

// Signal constants are canonically defined in `crate::signals` (the semantic
// owner). Re-exported here so existing `use crate::value::fiber::SIG_*`
// imports continue to work.
pub use crate::signals::{
    SIG_ABORT, SIG_DEBUG, SIG_ERROR, SIG_EXEC, SIG_FFI, SIG_FUEL, SIG_HALT, SIG_IO, SIG_OK,
    SIG_PROPAGATE, SIG_QUERY, SIG_RESUME, SIG_SWITCH, SIG_TERMINAL, SIG_WAIT, SIG_YIELD,
};

/// Fiber lifecycle status. Diverges from Janet: caught SIG_ERROR leaves
/// fiber Suspended (resumable), not Error. See vm/fiber.rs for details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiberStatus {
    /// Not yet started (has closure but hasn't been resumed)
    New,
    /// Currently executing (on the VM's run stack)
    Alive,
    /// Paused by a signal (waiting for resume)
    Paused,
    /// Completed normally (returned a value)
    Dead,
    /// Terminated by an unhandled error signal
    Error,
}

impl FiberStatus {
    /// Human-readable name for display formatting.
    pub fn as_str(self) -> &'static str {
        match self {
            FiberStatus::New => "new",
            FiberStatus::Alive => "alive",
            FiberStatus::Paused => "paused",
            FiberStatus::Dead => "dead",
            FiberStatus::Error => "error",
        }
    }
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

/// Maximum non-tail call depth before emitting a catchable stack-overflow
/// error.
///
/// Regular Elle→Elle calls push frames onto the Fiber's heap-resident call
/// stack (`call_closure_inner`) and the dispatch loop continues — the Rust
/// stack does not recurse.  Only `execute_bytecode_saving_stack` (used for
/// `run_on` and native re-entry) actually recurses on the Rust stack.
///
/// Because native re-entry paths share this counter, the limit must stay
/// below what would overflow the default 8 MB Rust thread stack (~4K–8K
/// frames of `execute_bytecode_saving_stack`).  10,000 is well above any
/// legitimate recursion depth while staying safely below the hard crash.
///
/// Tail calls bypass this check entirely — they reuse the current frame.
///
/// Shared by the interpreter (`vm::call`) and JIT (`jit::calls`) paths.
pub const MAX_CALL_DEPTH: usize = 1_000_000;

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
    /// Call frame stack (for fiber execution — closure + ip + base)
    pub frames: Vec<Frame>,
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
    /// Recorded `(param_id, region, generation)` for the heap values in this
    /// fiber's *inherited baseline* parameter frame — the uncounted cross-fiber
    /// borrows a child fiber snapshots from its parent (e.g. the scheduler a
    /// fiber reaches via a dynamic parameter). Seeding the baseline takes no
    /// reference count, so the borrow is sound only while the region stays live;
    /// the recorded generation lets the resume and `resolve_parameter` checks
    /// confirm that (debug builds), turning a violated invariant into a panic at
    /// the borrow instead of a stale read. Populated only under
    /// `debug_assertions`; empty otherwise (docs/impl/region-generations.md
    /// § "Uncounted-borrow check").
    pub param_borrows: Vec<(u32, crate::hir::region::RuntimeRegion, u32)>,
    /// Signal value from this fiber. Canonical location for both
    /// signal payloads and normal return values.
    /// - On signal: (bits, payload) before suspending
    /// - On normal return: (SIG_OK, return_value) before completing
    pub signal: Option<(SignalBits, Value)>,
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

    /// Per-activation region-slot remap (docs/impl/region-model.md — every value its
    /// own region). Each entry maps a static bytecode region id (a per-
    /// function "slot") to a fresh physical region id minted for *this*
    /// activation. The stack mirrors the call stack: the top is the current
    /// activation's frame; a closure call pushes a fresh frame, a normal
    /// return pops it, a tail call reuses it. Always non-empty (a base frame
    /// covers the top level). Carried on the fiber so it survives yields.
    pub activation_region_maps: Vec<rustc_hash::FxHashMap<u32, crate::hir::region::RuntimeRegion>>,

    /// The per-activation OWNER-NODE slots, parallel to `activation_region_maps`
    /// (one entry per activation frame; pushed/popped only through
    /// `VM::push_activation_region_map` / `restore_activation_region_map` /
    /// `pop_activation_region_map`, which keep the two stacks in lockstep). An
    /// entry holds the activation's pages-less owner-node region — the forest
    /// root `AdoptIntoActivation` adopts members into (docs/impl/region-model.md
    /// § "Owner nodes — an activation as a forest root") — or `None` until the
    /// activation's first adopt lazily mints it. Freed at the activation's
    /// normal completion (`VM::release_activation_owner_node`); a suspend MOVES
    /// the slot's node into the parked frame
    /// ([`BytecodeFrame::activation_owner_node`]) and the resume restores it,
    /// so the node reaches that completion across any number of parks.
    pub activation_owner_nodes: Vec<Option<crate::hir::region::RuntimeRegion>>,

    /// The FIBER's own owner node — the pages-less forest root for a region whose
    /// owner is the fiber itself, outliving every single activation
    /// (docs/impl/region-model.md § "Owner nodes" — "The fiber owner node").
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
    /// Snapshotted/restored like `activation_region_maps`: a nested call saves
    /// and restores it around the callee (`execute_bytecode_saving_stack`), a
    /// tail call re-installs it on the frame replacement (`trampoline_loop`), and
    /// a yield parks it in the `BytecodeFrame` and restores it on resume — so it
    /// is per-activation and rides fiber swaps with the fiber, never a VM-global
    /// read across a switch. `NIL` when no closure is executing (the top-level
    /// body) or when an entrant left it untracked.
    pub current_closure: Value,

    // --- Execution state migrated from VM ---
    /// Call depth counter (for stack overflow detection)
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

/// Create a minimal no-op closure for native iterator fibers.
/// The bytecode is a single Return instruction (opcode 3) which
/// is never actually executed — native iter fibers short-circuit
/// in the VM's resume path.
fn noop_closure() -> Rc<Closure> {
    use crate::value::closure::ClosureTemplate;
    use crate::value::types::Arity;

    Rc::new(Closure {
        template: crate::value::TemplateRef::new(Rc::new(ClosureTemplate {
            ..ClosureTemplate::new(
                Rc::new(vec![3, 0, 0, 0]), // Return
                Arity::Exact(0),
                Rc::new(vec![]),
            )
        })),
        // The no-op closure captures nothing — an empty env slice needs no
        // region (no allocation), so build it directly (the empty env needs no allocation).
        env: crate::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    })
}

impl Fiber {
    /// Create a new fiber from a closure with the given signal mask.
    pub fn new(closure: Rc<Closure>, mask: SignalBits) -> Self {
        Fiber {
            stack: SmallVec::new(),
            frames: Vec::new(),
            status: FiberStatus::New,
            mask,
            parent: None,
            parent_value: None,
            child: None,
            child_value: None,
            closure,
            closure_value: Value::NIL,
            param_frames: Vec::new(),
            param_borrows: Vec::new(),
            signal: None,
            suspended: None,
            activation_region_maps: vec![rustc_hash::FxHashMap::default()],
            activation_owner_nodes: vec![None],
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
    pub fn native_iter(elements: Vec<Value>, mask: SignalBits) -> Self {
        let closure = noop_closure();
        Fiber {
            stack: SmallVec::new(),
            frames: Vec::new(),
            status: FiberStatus::Paused,
            mask,
            parent: None,
            parent_value: None,
            child: None,
            child_value: None,
            closure,
            closure_value: Value::NIL,
            param_frames: Vec::new(),
            param_borrows: Vec::new(),
            signal: None,
            suspended: None,
            activation_region_maps: vec![rustc_hash::FxHashMap::default()],
            activation_owner_nodes: vec![None],
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
    /// caller-supplied `region` (Rule 3; docs/impl/region-ctx.md).
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
            self.frames.len(),
            self.stack.len()
        )
    }
}

#[cfg(test)]
mod tests;
