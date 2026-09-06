// audited: 2026-09-05
//! Suspension and call-frame types: `BytecodeFrame` (a parked bytecode
//! execution point), `SuspendedFrame` (one step in a fiber's replay chain),
//! and the execution/stack-trace frames `Frame` / `CallFrame`.
//!
//! docs/impl/region/generations.md
//! docs/impl/region/owner.md

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
    pub activation_region_map: rustc_hash::FxHashMap<u32, crate::hir::region::MappedRegion>,
    /// What this activation owed at the moment of suspension — MOVED (taken,
    /// never cloned) out of the activation's `Fiber::activation_dues` slot by
    /// the suspend site, so the record lives in exactly one place: the live slot
    /// or one parked frame (docs/impl/region/owner.md § "Owner nodes" — "A park
    /// moves the node into the suspended frame"). The owner node's members are
    /// `Owned` (RC frozen) with no other release route, and the deferred set's
    /// regions have no route at all — their emitting instruction died with the
    /// frame the tail call replaced — so both must ride the park to the resumed
    /// body's completion, where the trampoline's clean-break release runs them.
    /// `resume_suspended` restores the record into the live slot beside
    /// `activation_region_map`. Default (no node, nothing deferred) for an
    /// activation that neither adopted nor tail-called.
    pub activation_dues: crate::value::fiber::ActivationDues,
    /// The executing-closure register (`Fiber::current_closure`) at the moment this
    /// activation suspended. A yield unwinds the Rust call stack and restores the
    /// live register to the caller's value, so without parking it here a self-edge
    /// resolved after resume would name the wrong closure. `resume_suspended`
    /// re-installs it before re-entering the body. An uncounted borrow that may be
    /// DEAD by resume time — the region solver frees a closure value at its last
    /// use while the activation's `code`/`env` live on as `Rc`s — so it is never
    /// dereferenced on restore; it is live exactly where it is read (`LoadSelf`,
    /// whose self-recursive body keeps its closure region alive through the
    /// recursion). The JIT side-exit helpers park the executing closure here
    /// too (`src/jit/suspend.rs`); `NIL` only for an untracked activation.
    pub current_closure: Value,
    /// Debug-only snapshot of the uncounted region borrows `activation_region_map`
    /// holds at suspension: `(slot, region, establish-generation)` per LIVE mapped
    /// region. The suspended-frame analogue of the cross-fiber param snapshot's
    /// `param_borrows` (docs/impl/region/generations.md § "Two borrow shapes"). The
    /// snapshotted regions are this activation's own live allocations, kept alive by
    /// its still-pending `DecrefRegion`s while the fiber is parked; a map slot whose
    /// region's generation has already moved on is a dead leftover (see
    /// `record_region_borrows`) and is skipped, not recorded. `resume_suspended`
    /// re-checks each recorded generation before `restore_activation_region_map`, so
    /// a live borrow freed while parked panics at the resume boundary (naming the
    /// slot) instead of corrupting the resumed activation's allocs/decrefs. Filled by
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
        activation_region_map: rustc_hash::FxHashMap<u32, crate::hir::region::MappedRegion>,
        activation_dues: crate::value::fiber::ActivationDues,
        current_closure: Value,
        heap: &crate::value::fiberheap::FiberHeap,
    ) -> Self {
        #[cfg(debug_assertions)]
        let region_borrows =
            record_region_borrows(&activation_region_map, heap, code.frame_release_regions());
        #[cfg(not(debug_assertions))]
        let _ = heap;
        BytecodeFrame {
            code,
            env,
            ip,
            stack,
            push_resume_value,
            activation_region_map,
            activation_dues,
            current_closure,
            #[cfg(debug_assertions)]
            region_borrows,
        }
    }
}

/// Snapshot the uncounted region borrows a suspended bytecode frame's
/// `activation_region_map` holds: for each live `(slot, region)`, record
/// `(slot, region, establish-generation)`. The suspended-frame analogue of
/// `record_param_borrows` (the cross-fiber param snapshot, `src/vm/fiber.rs`);
/// the recorded generation lets `resume_suspended`'s `first_stale_borrow`
/// confirm the region has not been freed since the fiber parked.
///
/// A slot whose recorded `MappedRegion::gen` no longer matches the region's
/// current generation is a **dead leftover** — the region the activation
/// established there was freed by a non-slot-clearing path (a value-based drop,
/// a cross-region cascade, a subtree drop) and its physical id recycled to an
/// unrelated region. That slot is not a borrow the activation still holds, so it
/// is skipped: recording it would forge a borrow of a region this activation
/// never owned and trip the resume check on an unrelated incarnation's free (the
/// stale-suspended-frame false positive; docs/impl/region/generations.md
/// § "Uncounted-borrow check"). A slot whose generation still matches is a
/// genuine live borrow; recording its establish-generation means a free of *that*
/// incarnation while the fiber is parked still trips the check. The region and
/// its generation are read from the SAME `heap`, so the recorded pair and the
/// later check compare within one store. Debug builds only
/// (docs/impl/region/generations.md § "Two borrow shapes").
#[cfg(debug_assertions)]
pub(crate) fn record_region_borrows(
    map: &rustc_hash::FxHashMap<u32, crate::hir::region::MappedRegion>,
    heap: &crate::value::fiberheap::FiberHeap,
    slot_routed: &[u32],
) -> Vec<(u32, crate::hir::region::RuntimeRegion, u32)> {
    let _ = slot_routed;
    map.iter()
        .filter(|(_, m)| heap.generation_raw(m.region.get()) == m.gen)
        .map(|(&slot, m)| (slot, m.region, m.gen))
        .collect()
}

/// Where a suspended frame parked, for the resume-boundary panic a stale
/// uncounted borrow raises (docs/impl/region/generations.md).
pub struct ParkSite<'a> {
    /// The parked activation's function, `None` for an anonymous body.
    pub function: Option<&'a str>,
    /// The location recorded for the resume ip, where the table has one.
    pub at: Option<crate::reader::SourceLoc>,
    /// The function's first recorded location, which names the file where the
    /// resume ip has no entry of its own.
    pub start: Option<crate::reader::SourceLoc>,
    pub ip: usize,
    /// This frame's index in the replay chain, and the chain's length.
    pub frame: usize,
    pub frames: usize,
}

impl<'a> ParkSite<'a> {
    /// Read the site off the frame's own code object and resume ip. `frame` is
    /// the index `resume_suspended` is replaying and `frames` the chain's
    /// length, which together say how far into the replay the park sits.
    pub fn of(code: &'a crate::value::Code, ip: usize, frame: usize, frames: usize) -> Self {
        let locations = code.locations();
        ParkSite {
            function: code.template().name(),
            at: locations.get(ip),
            start: locations.first(),
            ip,
            frame,
            frames,
        }
    }

    /// The resume-boundary panic text for a stale suspended-frame region borrow.
    ///
    /// The slot and the physical region say which borrow died; the site says
    /// which program held it. Both halves are needed, and the second is the one
    /// a reader who cannot run the failing machine has to work from
    /// (docs/impl/region/generations.md).
    pub fn stale_borrow_message(
        &self,
        slot: u32,
        region: crate::hir::region::RuntimeRegion,
    ) -> String {
        // An exact-offset table lookup misses whenever the resume point is not
        // itself a recorded offset, so fall back to the line the function starts
        // on: naming the file is most of what the reader needs.
        let where_ = match (&self.at, &self.start) {
            (Some(at), _) => format!(" at {at}"),
            (None, Some(start)) => format!(" somewhere below {start}"),
            (None, None) => String::new(),
        };
        format!(
            "stale suspended-frame region borrow on resume: activation region \
             slot {slot} maps to region {}, which was freed while this fiber was \
             parked — an uncounted suspended-frame borrow outlived its region \
             (docs/impl/region/generations.md § 'Uncounted-borrow check')\
             \n  parked in {}{where_}, frame {} of {}, ip {}",
            region.get(),
            self.function.unwrap_or("<anonymous>"),
            self.frame,
            self.frames,
            self.ip,
        )
    }
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

/// Call frame for stack traces (code object + ip + frame_base).
/// Separate from Frame because stack traces need the function's name and the
/// source location of its current instruction, while execution dispatch needs
/// closure references.
/// A trace line names the function that was entered and the place it was called
/// from, so the frame holds both code objects. Holding them (rather than copying
/// a name and a location table out at push time) is also what keeps their
/// payloads alive for as long as the trace can be printed.
#[derive(Debug, Clone)]
pub struct CallFrame {
    /// The code object this frame entered — where the trace line's name comes
    /// from.
    pub callee: crate::value::Code,
    /// The code object that made the call. With `ip`, it gives the trace line's
    /// source location.
    pub caller: crate::value::Code,
    /// The offset of the call instruction in `caller`.
    pub ip: usize,
    pub frame_base: usize,
}

impl CallFrame {
    /// The entered function's name for a trace line.
    pub fn name(&self) -> &str {
        self.callee.template().name().unwrap_or("<anonymous>")
    }

    /// Where the call was made.
    pub fn location(&self) -> Option<crate::reader::SourceLoc> {
        self.caller.locations().get(self.ip)
    }
}
