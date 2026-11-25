// audited: 2026-11-24
//! The macro-expansion scope: its open, the transformer result's release, and the close that reclaims the scratch.
//!
//! docs/impl/region/macroscope.md
//! docs/impl/region/template.md
//! docs/impl/region/model.md

use super::*;
use crate::value::fiberheap::regionstore::RegionMint;

/// One open macro-expansion scope: the transient region the expansion wraps its
/// arguments into, held as the mint receipt that returns the region's physical
/// id (docs/impl/region/model.md).
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

    /// Give back the one reference the transformer call handed over with
    /// `result`, once the expander has copied it.
    ///
    /// The expander is the call's caller, so this is the `DecrefValueRegion` a
    /// compiled caller runs, resolved as [`release_program_value`] resolves it.
    /// The close cannot stand in for it, because it balances only the scope's
    /// own mints. The result may live in a region minted outside the scope,
    /// such as a constant the hoist moved into the transformer's unit. An
    /// immediate has no region, so this is a no-op for one.
    pub fn release_result(&self, heap: &mut FiberHeap, result: Value) {
        let region = result_region_of(heap, result);
        decref_region(heap, region);
    }
}

/// Take the reference a transformer's compile left on `transformer`'s region
/// for the transformer cell that keeps the closure. A scope's reclaim cannot
/// see a holder in a Rust structure, so the hold is recorded, and a scope the
/// compile ran inside leaves the region alone.
pub fn hold_transformer(heap: &mut FiberHeap, transformer: Value) {
    if let Some(region) = region_of(heap, transformer) {
        heap.note_rust_hold(region);
    }
}

/// Give back the transformer cell's reference to `transformer`'s region.
pub fn release_transformer(heap: &mut FiberHeap, transformer: Value) {
    let region = region_of(heap, transformer);
    if let Some(r) = region {
        heap.drop_rust_hold(r);
    }
    decref_region(heap, region);
}

/// Open a macro-expansion allocation scope. Every region minted until the
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
/// - the code-hold registry, which names every code region a `CodeUnit` or a
///   `CodePin` holds: a unit compiled inside the expansion, or a JIT entry a
///   call inside the transformer submitted.
///
/// Excluding them delays no reclamation, because each answers to its own
/// owner: teardown for a process root, the drop of its last Rust handle for a
/// code region.
///
/// The transient argument region's physical id comes back here, and the recycle
/// runs FIRST so it reads the region as the expansion left it. An expansion that
/// wrapped nothing left it unmaterialized, and no teardown can ever return an id
/// that names no region. One that wrapped an argument left a live region the
/// reclaim below frees, and that teardown books the id. So the recycle reads it
/// live and pushes nothing (docs/impl/region/model.md).
pub fn reclaim_macro_scope(heap: &mut FiberHeap, scope: MacroScope) {
    heap.recycle_unmaterialized_region(scope.arg);
    let mut protected = heap.process_roots_snapshot();
    if let Some(root) = heap.root_region_slot() {
        protected.push(root);
    }
    protected.extend(heap.code_hold_regions());
    heap.reclaim_region_mint_scope(&protected);
}
