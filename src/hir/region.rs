//! Region types for per-value region inference.
//!
//! Every allocation site gets a unique region variable. The solver
//! widens variables through a tree lattice (GLOBAL at the root,
//! innermost scope at the leaves). After solving, each allocation
//! knows its death point — the scope whose exit frees it.
//!
//! `RegionInfo` is the solver's output: per-allocation region
//! assignments and the set of regions that contain live allocations.
//! The lowerer queries `scope_has_local_allocs(hir_id)` to decide
//! whether a scope gets RegionEnter/RegionExit.

use super::binding::Binding;
use super::expr::HirId;
use crate::value::SymbolId;

use rustc_hash::FxHashSet;
use std::collections::HashMap;

/// The integer type used for region IDs in bytecode and at runtime.
/// Change this single definition to widen region IDs system-wide.
pub type RegionId = u16;

/// Call classification data for region inference.
///
/// Tells the region inference walk which calls return immediates
/// (no heap allocation) so their results don't need alloc_vars.
/// Without this, every call inside a scope prevents scope reclamation.
#[derive(Default, Clone)]
pub struct CallClassification {
    /// Intrinsic SymbolIds (BinOp, CmpOp, etc.) — return immediates.
    pub intrinsic_ops: FxHashSet<SymbolId>,
    /// Primitive SymbolIds with `returns_immediate: true` in PrimitiveDef.
    pub immediates: FxHashSet<SymbolId>,
    /// Primitive SymbolIds with `escapes_args: true` in PrimitiveDef.
    /// The solver widens their heap arguments to the enclosing region.
    pub escapers: FxHashSet<SymbolId>,
    /// Letrec-bound Bindings whose lambda bodies return immediates.
    /// Populated by the callee fixpoint pre-pass.
    pub user_immediates: FxHashSet<Binding>,
}

/// A region identifier assigned by the solver.
///
/// Every allocation site gets a unique Region from the solver.
/// Region IDs start at 1. Region(0) is invalid — it means "no region
/// assigned" and must panic if encountered in an allocation path.
///
/// There are no special-cased region constants. The outermost region
/// of a compilation unit is just the first region the solver creates.
/// All regions are treated uniformly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region(pub u32);

impl Region {
    /// Convert to runtime region ID. Panics if the region ID is 0
    /// (invalid) or exceeds the representable range.
    pub fn to_runtime_id(self) -> RegionId {
        assert!(self.0 > 0, "Region(0) is invalid — solver bug");
        assert!(
            self.0 <= RegionId::MAX as u32,
            "region ID {} exceeds RegionId range",
            self.0
        );
        self.0 as RegionId
    }
}

/// An outlives constraint: `shorter` must be widened to at least
/// `longer` in the region tree.
#[derive(Debug)]
pub struct OutlivesConstraint {
    /// Region variable that must live at least as long
    pub longer: u32,
    /// Region variable that may need widening
    pub shorter: u32,
    /// HIR node that generated this constraint (for diagnostics)
    pub source: HirId,
}

/// Per-region metadata produced by region inference.
///
/// `free_at` is the HirId at which the lowerer emits the region's
/// compiler-owned `DecrefRegion`. Every region has exactly one
/// `free_at` — there is no `Option<HirId>`. If the region's value
/// has no use anywhere, `free_at` equals the allocation HirId
/// itself (decref fires immediately after the alloc).
#[derive(Debug, Clone)]
pub struct RegionData {
    pub free_at: HirId,
}

/// Results of region inference for a compilation unit.
///
/// Every allocation site has a solved region in `alloc_region`.
/// Every scope (Let, Letrec, Block, Loop, Lambda) has a region in
/// `scope_region`. A scope is reclaimable when its region appears
/// in `live_regions` (at least one allocation's death point is
/// that scope's exit).
pub struct RegionInfo {
    /// HirId → solved region for each allocation site.
    pub alloc_region: HashMap<HirId, Region>,
    /// HirId → region introduced by each scope node.
    pub scope_region: HashMap<HirId, Region>,
    /// Binding → region where the binding lives.
    pub binding_region: HashMap<Binding, Region>,
    /// Binding → source regions the binding's value may point into.
    ///
    /// Populated from the inference's `binding_regions` after the walk;
    /// used by the `free_at` extension pass so a binding's last use
    /// keeps its source regions alive. Exposed publicly because tests
    /// pin the invariant that a side-effecting destructure inside a
    /// letrec init does not get its `binding_regions[b]` overwritten by
    /// a subsequent placeholder init (e.g. `r = nil`) for the same
    /// binding — without this, the source region's `free_at` is left
    /// at the destructure's id and a stale ptr survives to a later
    /// use of `r`, panicking in arena::deref.
    pub binding_source_regions: HashMap<Binding, Vec<Region>>,
    /// Regions that have at least one allocation assigned to them.
    pub live_regions: FxHashSet<Region>,
    /// Cross-region references detected by the solver.
    /// Each entry is (store_site, source_region, target_region) where a
    /// value in source_region is stored into a structure in target_region.
    /// The lowerer emits IncrefRegion(source) at the store site; the
    /// runtime cascade at `DecrefRegion(target)` releases the source
    /// reference when target's RC hits 0.
    pub cross_region_refs: Vec<(HirId, Region, Region)>,
    /// Per-region metadata (free_at HirId, etc.).
    ///
    /// Populated by region inference (impl step 11).
    pub region_data: HashMap<Region, RegionData>,
    /// Per-lambda HirId, the set of source regions that flow as the
    /// lambda body's tail return value. The lowerer uses this to
    /// suppress the compiler-emitted `DecrefRegion` for those regions
    /// inside the lambda's body — ownership of the initial RC=1 is
    /// transferred to the caller, which holds the value through the
    /// `Return` boundary. Without this transfer, the tail `DecrefRegion`
    /// would fire after the `LoadLocal` that loads the return value
    /// and before `Return`, producing a use-after-free.
    pub lambda_tail_regions: HashMap<HirId, Vec<Region>>,
    /// Set of regions created by `alloc_here` at a Call HirId. Their
    /// compile-time region ID is just a placeholder for the runtime
    /// region of the callee's returned value (which the caller cannot
    /// statically name). The lowerer emits `ReleaseValueRegion(reg)`
    /// at these regions' `free_at` instead of `DecrefRegion(rid)`,
    /// reading the runtime region from the value at last use.
    pub call_result_regions: FxHashSet<Region>,
    /// Statistics.
    pub stats: RegionStats,
}

impl RegionInfo {
    pub fn empty() -> Self {
        RegionInfo {
            alloc_region: HashMap::new(),
            scope_region: HashMap::new(),
            binding_region: HashMap::new(),
            binding_source_regions: HashMap::new(),
            live_regions: FxHashSet::default(),
            cross_region_refs: Vec::new(),
            region_data: HashMap::new(),
            lambda_tail_regions: HashMap::new(),
            call_result_regions: FxHashSet::default(),
            stats: RegionStats::default(),
        }
    }

    /// Does this scope have any allocations whose solved region matches it?
    pub fn scope_has_local_allocs(&self, hir_id: HirId) -> bool {
        self.scope_region
            .get(&hir_id)
            .is_some_and(|r| self.live_regions.contains(r))
    }
}

/// Statistics from region inference.
#[derive(Debug, Default)]
pub struct RegionStats {
    pub regions_created: usize,
    pub constraints_generated: usize,
    pub solver_iterations: usize,
    pub live_scopes: usize,
    pub empty_scopes: usize,
}

impl std::fmt::Display for RegionStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "region inference stats:")?;
        writeln!(
            f,
            "  regions: {}  constraints: {}  iterations: {}",
            self.regions_created, self.constraints_generated, self.solver_iterations
        )?;
        writeln!(
            f,
            "  live: {}  empty: {}",
            self.live_scopes, self.empty_scopes
        )?;
        Ok(())
    }
}
