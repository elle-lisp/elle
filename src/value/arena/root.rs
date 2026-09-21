// audited: 2026-09-20
//! Process roots, the program value's hand-off, the pinned root region, and the
//! macro-expansion scope.
//! docs/impl/region/rules.md
//! docs/impl/region/template.md
//! docs/impl/region/model.md

use super::*;
use crate::value::fiberheap::regionstore::RegionMint;

/// Record `region` as a process root of `heap` — a region the teardown sweep
/// will release (decref once) so its RC can reach zero and cascade. Idempotent
/// per id is the caller's responsibility; a region registered twice is decref'd
/// twice. The registry is instance-owned (a `FiberHeap` field), not thread-local.
pub fn register_process_root_region(heap: &mut FiberHeap, region: RuntimeRegion) {
    heap.register_process_root_region(region);
}
/// Record `value`'s region as a process root of `heap` (see
/// [`register_process_root_region`]). A value with no region (an immediate) is
/// ignored — the type-level form of "only heap values pin a region."
pub fn register_process_root(heap: &mut FiberHeap, value: Value) {
    if let Some(r) = region_of(heap, value) {
        heap.register_process_root_region(r);
    }
}
/// Give back the one owning reference a completed run handed its host with the
/// program value (docs/impl/region/rules.md § "The program value is the host's
/// to release"). The mirror of the `DecrefValueRegion` a compiled caller runs:
/// it resolves the value's own runtime region, seeing through a capture cell
/// exactly as the return convention's mint did.
///
/// A host that reads the value for the rest of the runtime's life registers it
/// with [`register_process_root`] instead, and the teardown sweep releases it.
/// An immediate has no region, so this is a no-op for one.
pub fn release_program_value(heap: &mut FiberHeap, value: Value) {
    let region = result_region_of(heap, value);
    decref_region(heap, region);
}

/// Release every registered process root of `heap` by reference count and return
/// the number released. This is the *only* heap-region action the teardown sweep
/// takes — it decrefs roots and lets the RC cascade do the rest (Rule 5/7); it
/// never iterates the region table freeing live entries (see
/// docs/impl/region/rules.md § "Teardown", property 1).
///
/// Draining the registry makes a second call a no-op, so teardown is idempotent.
pub fn teardown_process_root_regions(heap: &mut FiberHeap) -> usize {
    // Code payloads are released alongside the roots: nothing may still be
    // executing at teardown, so every payload is dead whatever its blueprint's
    // refcount says (docs/impl/region/template.md § "Who owns the payload
    // region"). Like a root, each is a decref — the RC cascade does the rest.
    heap.release_all_template_payloads();
    let roots = heap.take_process_roots();
    // The root region's slot is consumed here too: it was registered at mint, so
    // it is in `roots`; clearing the slot prevents a later mint from aliasing a
    // recycled id onto a stale handle.
    heap.set_root_region(None);
    let n = roots.len();
    for r in roots {
        heap.decref_region_if_present(r);
    }
    n
}
/// Mint-or-get `heap`'s pinned process-lifetime root region, registering it as a
/// process root on first use so teardown releases it by RC.
///
/// `pub(crate)` for the root values whose payload must be built in the same
/// region as their header — a slice-backed root cannot go through
/// [`alloc_root`] alone (docs/impl/region/model.md, "RegionSlice contents share
/// their object's region").
pub(crate) fn root_region(heap: &mut FiberHeap) -> RuntimeRegion {
    if let Some(r) = heap.root_region_slot() {
        return r;
    }
    let r = heap.new_runtime_region();
    heap.set_root_region(Some(r));
    // The root region is a process root: teardown releases it by RC like any
    // other region, rather than leaking it to instance exit.
    heap.register_process_root_region(r);
    r
}

/// One open macro-expansion scope: the transient region the expansion wraps its
/// arguments into, held as the mint receipt that returns the region's physical
/// id (docs/impl/region/model.md § "Physical id recycling").
///
/// The open hands this out rather than a bare `RuntimeRegion`, so the expander
/// cannot name the region without also holding what closes it. The close is
/// [`reclaim_macro_scope`], which consumes the scope.
#[must_use = "an unreclaimed macro scope strands the transient region's id and \
              leaves the transformer's scratch holding unbalanced references"]
pub struct MacroScope {
    arg: RegionMint,
}

impl MacroScope {
    /// The region this expansion's wrapped arguments are born in.
    pub fn arg_region(&self) -> RuntimeRegion {
        self.arg.region()
    }
}

/// Open a macro-expansion allocation scope (docs/impl/region/rules.md § "Macro
/// expansion — a closed allocation scope"). Every region minted until the
/// matching [`reclaim_macro_scope`] is recorded so its dead scratch can be
/// reclaimed by RC.
///
/// The transient argument region is minted here, inside the log, so the reclaim
/// covers it like any other region the expansion mints.
pub fn begin_macro_scope(heap: &mut FiberHeap) -> MacroScope {
    heap.begin_region_mint_log();
    MacroScope {
        arg: heap.new_runtime_region_tracked(),
    }
}

/// Close the scope opened by [`begin_macro_scope`] and reclaim the transformer's
/// dead scratch, excluding every region whose sole owner is held Rust-side.
///
/// The reclaim finds a region's holders through the heap-content in-degree
/// scan, so an owner living in a Rust structure is invisible to it and its
/// region reads as unreferenced scratch. Three owners are that, and a
/// transformer reaches each of them mid-scope:
///
/// - the process-root registry, which owns the trait method tables a
///   transformer's first trait dispatch (`append`'s `empty?`) allocates;
/// - the pinned root region those tables live in;
/// - the code-payload cache, which a `MakeClosure` inside the transformer
///   extends with a fresh region whenever the open one is full.
///
/// Excluding them delays no reclamation, because each answers to its own
/// owner: teardown for a process root, the death of the last blueprint packed
/// into it for a payload region.
///
/// The transient argument region's physical id comes back here, and the recycle
/// runs FIRST so it reads the region as the expansion left it. An expansion that
/// wrapped nothing left it unmaterialized, and no teardown can ever return an id
/// that names no region; one that wrapped an argument left a live region the
/// reclaim below frees, whose own teardown books the id, so the recycle reads it
/// live and pushes nothing (docs/impl/region/model.md § "Physical id recycling").
pub fn reclaim_macro_scope(heap: &mut FiberHeap, scope: MacroScope) {
    heap.recycle_unmaterialized_region(scope.arg);
    let mut protected = heap.process_roots_snapshot();
    if let Some(root) = heap.root_region_slot() {
        protected.push(root);
    }
    protected.extend(heap.template_payload_regions());
    heap.reclaim_region_mint_scope(&protected);
}

/// Allocate a startup-once process-lifetime root (the default trait-method
/// tables) into `heap`'s pinned root region — an ordinary mortal allocation
/// that, pinned by its holder (the trait registry) and never decref'd, persists
/// for the instance.
///
/// Safe with the un-scanned `traits` field: a value's `traits` pointer is not an
/// RC-tracked cross-region edge (`find_object_cross_refs` skips it for every
/// container, and the only object that does scan it — `Fiber` — never carries a
/// trait table), so no per-object alloc/free touches this region's RC; it stays
/// at its mint-time count for the whole run.
pub fn alloc_root(heap: &mut FiberHeap, obj: HeapObject) -> Value {
    let region = root_region(heap);
    heap.alloc_in_region(obj, region)
}
