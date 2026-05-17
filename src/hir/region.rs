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

/// Call classification data for region inference.
///
/// Tells the region inference walk which calls return immediates
/// (no heap allocation) so their results don't need alloc_vars.
/// Without this, every call inside a scope prevents scope reclamation.
#[derive(Default, Clone)]
pub struct CallClassification {
    /// Primitive SymbolIds known to return immediates.
    pub immediate_primitives: FxHashSet<SymbolId>,
    /// Intrinsic SymbolIds (BinOp, CmpOp, etc.) — also return immediates.
    pub intrinsic_ops: FxHashSet<SymbolId>,
    /// Letrec-bound Bindings whose lambda bodies return immediates.
    /// Populated by the callee fixpoint pre-pass.
    pub user_immediates: FxHashSet<Binding>,
}

/// A region identifier. Region(0) is GLOBAL (top of the lattice).
/// Other values are assigned by the constraint generator as scopes
/// are entered during the HIR walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Region(pub u32);

impl Region {
    /// The global region — allocations here are never reclaimed by
    /// scope or rotation instructions.
    pub const GLOBAL: Region = Region(0);

    pub fn is_global(self) -> bool {
        self == Self::GLOBAL
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
    /// Regions that have at least one allocation assigned to them.
    pub live_regions: FxHashSet<Region>,
    /// Statistics.
    pub stats: RegionStats,
}

impl RegionInfo {
    pub fn empty() -> Self {
        RegionInfo {
            alloc_region: HashMap::new(),
            scope_region: HashMap::new(),
            binding_region: HashMap::new(),
            live_regions: FxHashSet::default(),
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
