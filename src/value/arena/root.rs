use super::*;

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

/// Open a macro-expansion allocation scope (docs/impl/region/rules.md § "Macro
/// expansion — a closed allocation scope"). Every region minted until the
/// matching [`reclaim_macro_scope`] is recorded so its dead scratch can be
/// reclaimed by RC.
pub fn begin_macro_scope(heap: &mut FiberHeap) {
    heap.begin_region_mint_log();
}

/// Close the scope opened by [`begin_macro_scope`] and reclaim the transformer's
/// dead scratch — EXCLUDING process-lifetime roots. Trait method tables (and the
/// root region) are allocated through [`alloc_root`]/`root_region`, which mint
/// a real region whose sole owner is held Rust-side by the registry — invisible
/// to the heap-content in-degree scan the reclaim uses. A transformer that
/// dispatches a trait method (e.g. `append`'s `empty?`) can trigger that first
/// allocation mid-scope, so the root region must be protected from reclamation
/// or the trait tables would be freed under the running program. The instance's
/// process roots plus its root region are exactly that exclusion set.
pub fn reclaim_macro_scope(heap: &mut FiberHeap) {
    let mut protected = heap.process_roots_snapshot();
    if let Some(root) = heap.root_region_slot() {
        protected.push(root);
    }
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
