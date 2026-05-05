//! Region types for Tofte-Talpin region inference.
//!
//! Every scope (Let, Letrec, Block, Loop, Lambda) introduces a region — a
//! lifetime bucket. Every allocation site gets a region variable. Constraints
//! propagate through a lattice where GLOBAL is top and the innermost scope
//! is bottom. The solver widens variables monotonically toward GLOBAL.
//!
//! After solving:
//! - `alloc_region == scope_region` → RegionEnter/RegionExit (scope reclaim)
//! - `alloc_region ∈ loop_regions` → FlipEnter/FlipSwap/FlipExit (rotation)
//! - `alloc_region == GLOBAL` → no reclamation (status quo)

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

/// What kind of region a scope introduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    /// Let/Letrec/Block scope → RegionEnter/RegionExit
    Scope,
    /// Loop scope → FlipEnter/FlipSwap/FlipExit
    Loop,
    /// Lambda scope → escapes to caller
    Function,
    /// No reclamation
    Global,
}

/// An outlives constraint: `longer`'s region must outlive `shorter`'s region.
/// Equivalently, `shorter` must be widened to at least `longer` in the
/// region tree.
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
pub struct RegionInfo {
    /// HirId → assigned region for allocation sites
    pub alloc_region: HashMap<HirId, Region>,
    /// HirId → region introduced by a scope node (Let, Letrec, Block, Loop, Lambda)
    pub scope_region: HashMap<HirId, Region>,
    /// HirId → what kind of scope this is
    pub scope_kind: HashMap<HirId, RegionKind>,
    /// Binding → region where binding lives
    pub binding_region: HashMap<Binding, Region>,
    /// Statistics
    pub stats: RegionStats,
}

impl RegionInfo {
    /// Create an empty RegionInfo (no regions, no scopes).
    pub fn empty() -> Self {
        RegionInfo {
            alloc_region: HashMap::new(),
            scope_region: HashMap::new(),
            scope_kind: HashMap::new(),
            binding_region: HashMap::new(),
            stats: RegionStats::default(),
        }
    }
}

/// Statistics from region inference.
#[derive(Debug, Default)]
pub struct RegionStats {
    pub regions_created: usize,
    pub constraints_generated: usize,
    pub solver_iterations: usize,
    pub scopes_scope: usize,
    pub scopes_loop: usize,
    pub scopes_function: usize,
    pub scopes_global: usize,
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
            "  scope: {}  loop: {}  function: {}  global: {}",
            self.scopes_scope, self.scopes_loop, self.scopes_function, self.scopes_global
        )?;
        Ok(())
    }
}
