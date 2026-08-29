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
mod frame;
mod handle;
mod status;
pub use frame::*;
pub use handle::*;
pub use status::*;

mod signalbits;
pub use signalbits::SignalBits;

// Signal constants are canonically defined in `crate::signals` (the semantic
// owner). Re-exported here so existing `use crate::value::fiber::SIG_*`
// imports continue to work.
pub use crate::signals::{
    SIG_ABORT, SIG_DEBUG, SIG_ERROR, SIG_EXEC, SIG_FFI, SIG_FS, SIG_FUEL, SIG_HALT, SIG_IO, SIG_OK,
    SIG_PROPAGATE, SIG_QUERY, SIG_RESUME, SIG_SWITCH, SIG_TERMINAL, SIG_WAIT, SIG_YIELD,
};

/// Maximum non-tail call depth before emitting a stack-overflow halt
/// (`SIG_HALT`).
///
/// Every non-tail Elle→Elle closure call recurses on the Rust stack
/// (`call_inner` → `execute_bytecode_saving_stack`), costing ~25–30 KB per
/// level (dominated by the `SmallVec<[Value; 256]>` stack-save buffer). With
/// the default 8 MB thread stack the hard crash (SIGABRT) limit is ~280–310
/// levels, so the guard sits well below that — leaving headroom for the call
/// chain above user code (compilation, dispatch loop, primitives) and for
/// platforms with smaller default stacks. A larger constant here is a lie:
/// the process aborts on Rust stack exhaustion long before the counter trips
/// (integration::repl_exit_codes::test_stack_overflow_exits_with_error).
///
/// Tail calls bypass this check entirely — they are trampolined in
/// `execute_bytecode_saving_stack`'s loop and never grow the Rust stack.
///
/// Shared by the interpreter (`vm::call`) and JIT (`jit::calls`) paths.
pub const MAX_CALL_DEPTH: usize = 200;

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
    /// True once an inherited parameter BASELINE was installed as
    /// `param_frames[0]` (creation-time snapshot in `prim_fiber_new`, or the
    /// first-resume fallback). The seed retains each heap entry's region and
    /// records a `fiber → value` content edge; the Fiber content-scan arm
    /// visits the baseline exactly when this is set, so the fiber's free
    /// cascade is the symmetric release (docs/impl/region/owner.md § "A
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
    /// describes the error being re-raised — representation identity, as for
    /// `emit_delivery`, never structural equality. A fiber that goes on to
    /// park a different error (an injected abort, a second raise that recorded
    /// no location) therefore lends its old location to nothing.
    ///
    /// Like `emit_delivery`, the payload is an UNCOUNTED marker: it is only
    /// ever compared bit-wise, never dereferenced, so it takes no retain and
    /// the Fiber content scan records no edge for it. The counted edge for the
    /// same value is `signal`'s.
    pub error_loc: Option<(Value, crate::error::SourceLoc)>,
    /// The `SIG_ERROR` payload whose DELIVERY reference something OTHER than this
    /// fiber's frames minted. Three raises record here, for one reason:
    ///
    /// - an `emit` raise — the `EmitEscape` retain `handle_emit` (and its JIT
    ///   mirror) takes, which the resumer's release of the resume result consumes;
    /// - the same raise leaving the emit PRIMITIVE in tail position, where the
    ///   signal exit takes that retain in `handle_emit`'s place
    ///   (`VM::mint_raised_argument_delivery`);
    /// - an injected `fiber/abort` / `fiber/refuse` payload — the `AbortDelivery`
    ///   retain the injection takes, recorded on the aborted fiber
    ///   (`do_fiber_abort`) and on the aborting one where the error escapes it
    ///   (`VM::park_propagating_abort`).
    ///
    /// While this matches the live signal's payload (representation identity,
    /// never structural equality), the payload exemption on the abandoned-frame
    /// walk and the parked frame's discharge is withdrawn: a frame's own
    /// reference funds no delivery, so every release the tables name is genuinely
    /// owed (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
    /// it still owes"). A native install leaves this untouched — its delivery is
    /// the payload's birth reference or the frame's left-standing one, which the
    /// exemption preserves. Cleared at the resume's signal take, where the parked
    /// payload this record named leaves the slot.
    pub emit_delivery: Option<Value>,
    /// Whether this fiber's innermost suspension is a PRIMITIVE call, whose
    /// resume value therefore arrives owing one reference.
    ///
    /// A parked frame re-enters at its suspending call's continuation, which
    /// runs that call's compiler-emitted result release. A bytecode callee funds
    /// the reference that release consumes with its `Return` mint; a primitive
    /// that suspends never returns, so the delivery mints it instead
    /// (docs/impl/region/owner.md § "A delivery into a replayed frame carries one
    /// owning reference"). The classifier — `handle_primitive_signal` and the
    /// capability-denial path, in call and tail position and in their JIT twins
    /// (`src/jit/calls.rs`) — is the only place that knows which shape a park has,
    /// and the park itself may be built later
    /// and elsewhere (a tail suspend leaves no frame of its own), so the answer
    /// rides the fiber rather than the frame. `do_fiber_resume_single` takes it
    /// with the parked signal, so every delivery route consumes it exactly once
    /// and a later park of a different shape starts from `false`.
    pub resume_value_unfunded: bool,
    /// The CAPABILITY-DENIAL payload this fiber has parked, if its innermost
    /// suspension is a denial.
    ///
    /// A park leaves two references on its payload's region — the delivery, and
    /// the body's own, released by the continuation past the suspend — but a
    /// denial's `{:error :capability-denied …}` struct is built by the denial
    /// path, so the body never names it and no `decref_point` names its region.
    /// The reference the allocation left is owed by whatever displaces the
    /// payload: a resume's delivery, or an abort's / refusal's injected error
    /// (docs/impl/region/owner.md § "Park/unpark symmetry" — "A payload the
    /// RUNTIME built is released by the install that displaces it").
    ///
    /// A record is needed because the bits cannot say so. An io park is its
    /// `SIG_IO` bit, but a denial parks under the WITHHELD capability's bits,
    /// which an `(emit :fs v)` of a body-allocated value carries too — and only
    /// the classifier (`handle_capability_denial`, its tail twin, and the JIT's
    /// `jit_capability_denial`) knows which of the two it built.
    ///
    /// Like `emit_delivery`, this is an UNCOUNTED marker: only ever compared
    /// bit-wise against the parked signal, never dereferenced, so it takes no
    /// retain and the Fiber content scan records no edge for it. The counted
    /// edge for the same value is `signal`'s. The comparison is what bounds a
    /// stale record — a record that no longer names the parked payload releases
    /// nothing — and the displacing install TAKES it, so no second install can
    /// release the same reference.
    pub denial_payload: Option<Value>,
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

    /// The per-activation OWNER-NODE slots, parallel to `activation_region_maps`
    /// (one entry per activation frame; pushed/popped only through
    /// `VM::push_activation_region_map` / `restore_activation_region_map` /
    /// `pop_activation_region_map`, which keep the two stacks in lockstep). An
    /// entry holds the activation's pages-less owner-node region — the forest
    /// root `AdoptIntoActivation` adopts members into (docs/impl/region/owner.md
    /// § "Owner nodes — an activation as a forest root") — or `None` until the
    /// activation's first adopt lazily mints it. Freed at the activation's
    /// normal completion (`VM::release_activation_owner_node`); a suspend MOVES
    /// the slot's node into the parked frame
    /// ([`BytecodeFrame::activation_owner_node`]) and the resume restores it,
    /// so the node reaches that completion across any number of parks.
    pub activation_owner_nodes: Vec<Option<crate::hir::region::RuntimeRegion>>,

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

/// The parked region state a fiber that can never run again strands, TAKEN out
/// of the fiber so exactly one release path reaches it: the parked chain's
/// activation owner nodes, and the park escape retain on the parked signal's
/// value (otherwise released only on the resume path). Consumed by the
/// terminal-fiber teardown (`vm::fiber::release_fiber_owned`) and by the region
/// free path's fiber discharge (`RegionStore::teardown_set`)
/// (docs/impl/region/owner.md § "Park/unpark symmetry").
pub struct ParkedState {
    /// Each still-parked `BytecodeFrame`'s activation owner node, in chain order.
    /// The activation-map regions are deliberately NOT collected: a mapped slot
    /// can be stale (its value's release was emitted value-based, or died past a
    /// tail call), so a blanket map release double-frees a possibly-recycled id.
    pub nodes: Vec<crate::hir::region::RuntimeRegion>,
    /// The parked NON-TERMINAL signal (a yielded value, a yielding io request, a
    /// capability-denial payload) — its park took exactly one escape retain
    /// (`EmitEscape` / `SuspendEscape`) whose symmetric release lives on the
    /// resume path the fiber will never take.
    ///
    /// `None` when the parked signal is TERMINAL. A terminal signal keeps its
    /// slot for `fiber/value`, and the one retain pinning it — the park retain
    /// (`incref_signal_region`) — is the free-time signal scan's to release.
    ///
    /// Reporting a terminal signal here as well is not a second discharge to be
    /// had, it is an over-free: a terminal signal reaches the slot by paths that
    /// take no escape retain at all (a native error's `set_error`, a bare
    /// `Return`), and releasing one they never took frees a live region — the
    /// `elle test` harness dies on its own first file. What a terminal EMIT
    /// owes — the raise chain's own reference to a payload it allocated — is
    /// settled through [`Self::protect`] instead: the emit records its minted
    /// delivery (`Fiber::emit_delivery`), the protection is withheld where the
    /// record matches, and the frames' owed-release tables carry the reference
    /// with their own receipts.
    pub signal: Option<(SignalBits, Value)>,
    /// The values each still-parked `BytecodeFrame` owes a release for — read out
    /// of its saved locals at the slots its own `Code::frame_release_slots` names
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). A frame this fiber can never re-enter never reaches the
    /// `LoadLocal s; DecrefValueRegion; StoreLocal s nil` route that would have
    /// released them, so the one release each is owed runs at the discharge.
    ///
    /// This is the compiler's own release table, not the activation map: a mapped
    /// slot can be stale, which is why `nodes` above collects none of it, while a
    /// value-route slot carries its own receipt — the route stamps it nil, so a
    /// slot still holding a heap value is a release that did not run.
    ///
    /// The parked `signal`'s own value is excluded: a terminal payload is the
    /// fiber's result, read through `fiber/value`, and the free-time signal scan
    /// answers for it.
    pub owed: Vec<Value>,
    /// The same for the parked frames' **slot-routed** releases: a static region
    /// slot still mapped in a parked activation is a `DecrefRegion` that did not
    /// run. Carried with its establishing generation so a consumer can tell a live
    /// mapping from a leftover the frame's own release already answered for.
    pub owed_regions: Vec<crate::hir::region::MappedRegion>,
    /// The value the fiber's signal carries, if any — the payload a discharge
    /// must leave standing. A consumer skips an [`Self::owed`] entry living in
    /// this value's region: a terminal payload is the fiber's result and a
    /// non-terminal one is the [`Self::signal`] discharge's own, and a frame may
    /// well hold the very value the payload names.
    ///
    /// `None` also where the raise MINTED the payload's delivery itself
    /// (`Fiber::emit_delivery` matches the live signal): the frame's reference
    /// funds nothing there, so the owed-release tables run in full and the one
    /// reference the raise chain held is reclaimed rather than stranded
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
    /// it still owes").
    pub protect: Option<Value>,
}

impl Fiber {
    /// Take the parked region state of a fiber that can never run again — see
    /// [`ParkedState`]. Empties the fiber's `suspended` chain and, for a parked
    /// non-terminal signal this fiber OWNS, the `signal` slot, so no second
    /// release path can reach them.
    ///
    /// Ownership of the signal's park escape retain is read from the chain's
    /// innermost frame: a `Bytecode` frame means the suspend ran here (the
    /// retain was taken with this park); a `FiberResume` frame means the signal
    /// is a propagated VIEW of an awaited child's park — the child owns the
    /// retain, and releasing the view too would double-free the one retain
    /// across two discharges.
    pub fn take_parked_state(&mut self) -> ParkedState {
        let mut nodes = Vec::new();
        let mut owed = Vec::new();
        let mut owed_regions = Vec::new();
        let mut owns_signal = false;
        for (i, frame) in self
            .suspended
            .take()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            if let SuspendedFrame::Bytecode(f) = frame {
                if i == 0 {
                    owns_signal = true;
                }
                nodes.extend(f.activation_owner_node);
                // The releases this frame still owed. Its locals sit at the base
                // of the saved stack (the activation's own frame base, the stack
                // having been emptied at entry), so the emitter's slot indexes
                // address them directly.
                for slot in f.code.frame_release_slots.iter() {
                    match f.stack.get(*slot as usize) {
                        Some(v) if v.as_heap_ptr().is_some() => owed.push(*v),
                        _ => {}
                    }
                }
                // The slot-routed half: a static region slot still mapped in the
                // parked activation is a `DecrefRegion` that did not run, the
                // release having taken the mapping wherever it did. Named by the
                // frame's own function so a caller's leftovers past a tail call
                // stay out.
                for slot in f.code.frame_release_regions.iter() {
                    if let Some(m) = f.activation_region_map.get(slot) {
                        owed_regions.push(*m);
                    }
                }
            }
        }
        let signal = match self.signal {
            Some((bits, _)) if owns_signal && !crate::vm::fiber::is_terminal_signal(bits) => {
                self.signal.take()
            }
            _ => None,
        };
        // A discharged park is over, and the discharge below already runs its one
        // decref — so the record must not survive to a later resume of this fiber
        // (a hard kill leaves an `:error` fiber resumable). See
        // [`Fiber::denial_payload`].
        if signal.is_some() {
            self.denial_payload = None;
        }
        // The signal's payload leaves with the fiber's result — read through
        // `fiber/value`, or accounted by the signal discharge below — so a slot
        // naming its region is not this discharge's to release. Reported rather
        // than filtered here: the region behind a value is the heap's to resolve,
        // and both consumers have one. An emit-minted error payload is the
        // exception: its delivery was retained at the raise, so the frames'
        // owed releases run in full (see `protect`'s doc).
        let protect = signal
            .or(self.signal)
            .map(|(_, v)| v)
            .filter(|v| !self.emit_delivery.is_some_and(|m| m.bit_identical(*v)));
        ParkedState {
            nodes,
            signal,
            owed,
            owed_regions,
            protect,
        }
    }

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
            param_baseline_seeded: false,
            param_borrows: Vec::new(),
            signal: None,
            error_loc: None,
            emit_delivery: None,
            resume_value_unfunded: false,
            denial_payload: None,
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
            param_baseline_seeded: false,
            param_borrows: Vec::new(),
            signal: None,
            error_loc: None,
            emit_delivery: None,
            resume_value_unfunded: false,
            denial_payload: None,
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
            self.frames.len(),
            self.stack.len()
        )
    }
}

#[cfg(test)]
mod tests;
