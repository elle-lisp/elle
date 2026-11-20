// audited: 2026-11-19
//! Process roots, the program value's hand-off, and the pinned root region.
//!
//! docs/impl/region/rules.md
//! docs/impl/region/template.md
//! docs/impl/region/model.md

use super::*;

/// Record `region` as a process root of `heap` — a region the teardown sweep
/// will release (decref once) so its RC can reach zero and cascade. Idempotent
/// per id is the caller's responsibility; a region registered twice is decref'd
/// twice. The registry is instance-owned (a `FiberHeap` field), not thread-local.
pub fn register_process_root_region(heap: &mut FiberHeap, region: RuntimeRegion) {
    heap.register_process_root_region(region);
}
/// Which reference the process root a host registers is funded by
/// (docs/impl/region/rules.md). The sweep decrefs a registered region once, so
/// every registration answers this or it decrefs a reference nobody holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootRef {
    /// Take the one owning reference the caller holds and give nothing back.
    /// The program value of a completed run arrives this way.
    Take,
    /// Mint the root's own reference. The caller holds no reference to give:
    /// the value is reachable from one it holds elsewhere and releases, such as
    /// a leaf of the program value's trailing tuple.
    Mint,
}

/// Record `value`'s region as a process root of `heap` (see
/// [`register_process_root_region`]), funded as `funding` says. A value with no
/// region (an immediate) is ignored — the type-level form of "only heap values
/// pin a region."
///
/// The mint and the registration name one region, resolved once here, so a
/// minted reference cannot land on a region other than the one the sweep will
/// decref.
pub fn register_process_root(heap: &mut FiberHeap, value: Value, funding: RootRef) {
    let Some(r) = region_of(heap, value) else {
        return;
    };
    if funding == RootRef::Mint {
        incref_for_escape(heap, Some(r), EscapeSite::ProcessRoot);
    }
    heap.register_process_root_region(r);
}
/// Give back the one owning reference a completed run handed its host with the
/// program value (docs/impl/region/rules.md). The mirror of the
/// `DecrefValueRegion` a compiled caller runs: it resolves the value's own
/// runtime region, seeing through a capture cell exactly as the return
/// convention's mint did.
///
/// A host that reads the value for the rest of the runtime's life registers it
/// with [`register_process_root`] instead, taking the reference, and the
/// teardown sweep releases it. A host that keeps only a PART of the value —
/// the REPL, whose destructuring `def` binds the leaves of a tuple — mints a
/// reference for the part and still releases here. An immediate has no region,
/// so this is a no-op for one.
pub fn release_program_value(heap: &mut FiberHeap, value: Value) {
    let region = result_region_of(heap, value);
    decref_region(heap, region);
}

/// Release every registered process root of `heap` by reference count and return
/// the number released. This is the *only* heap-region action the teardown sweep
/// takes — it decrefs roots and lets the RC cascade do the rest (Rule 5/7); it
/// never iterates the region table freeing live entries
/// (docs/impl/region/rules.md).
///
/// Draining the registry makes a second call a no-op, so teardown is idempotent.
pub fn teardown_process_root_regions(heap: &mut FiberHeap) -> usize {
    let roots = heap.take_process_roots();
    // The root region's slot is consumed here too: it was registered at mint, so
    // it is in `roots`; clearing the slot prevents a later mint from aliasing a
    // recycled id onto a stale handle. The placeholder code object lives in that
    // region, so its slot goes with it.
    heap.set_root_region(None);
    heap.set_placeholder(None);
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
/// [`alloc_root`] alone (docs/impl/region/model.md).
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

/// Allocate a startup-once process-lifetime root (the default trait-method
/// tables) into `heap`'s pinned root region — an ordinary mortal allocation
/// that persists for the instance, because the process-root registration holds
/// the region's reference until teardown.
///
/// A value whose `traits` field names a root table holds a counted edge into
/// this region: `find_object_cross_refs` scans the field on every variant, so
/// the value's allocation increfs the region and its free decrefs it. Those
/// pairs balance, and the registration's own reference keeps the region alive
/// between them.
pub fn alloc_root(heap: &mut FiberHeap, obj: HeapObject) -> Value {
    let region = root_region(heap);
    heap.alloc_in_region(obj, region)
}
