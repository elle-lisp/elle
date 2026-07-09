//! Call-classification input to region inference: which calls return
//! immediates, and the per-primitive region effects/return types the walk
//! reads to shape its clique, containment, and result-region decisions.

use crate::hir::binding::Binding;
use crate::value::SymbolId;

use rustc_hash::FxHashSet;

/// Call classification data for region inference.
///
/// Tells the region inference walk which calls return immediates
/// (no heap allocation) so their results don't need alloc_vars.
/// Without this, every call inside a scope prevents scope reclamation.
#[derive(Default, Clone)]
pub struct CallClassification {
    /// Intrinsic SymbolIds (BinOp, CmpOp, etc.) — return immediates.
    pub intrinsic_ops: FxHashSet<SymbolId>,
    /// Primitive SymbolId → declared `RegionEffect` from `PrimitiveDef`
    /// (docs/impl/region/effects.md "Native region effects"). Keys the opaque-call
    /// arg clique (`Mixed`/absent → full mutual clique; `Stores` →
    /// directed edges from the listed args; the rest → no edges) and the
    /// immediate-result classification (`Immediate` → no result region).
    pub effects: rustc_hash::FxHashMap<SymbolId, crate::primitives::def::RegionEffect>,
    /// Primitive SymbolId → declared [`RetType`](crate::primitives::def::RetType).
    /// The ownership inference reads this to classify a `Funnel` store's container
    /// argument (a `MutableArray`/`MutableStruct` container retains the stored
    /// value's region — the forest recovers a containment edge there; see the
    /// `Funnel` arm in `regions::walk`). Empty by default — only the real
    /// primitive classification (`PrimitiveClassification::new`) fills it.
    pub ret_types: rustc_hash::FxHashMap<SymbolId, crate::primitives::def::RetType>,
    /// Primitive SymbolId → the 0-based argument indices the callee EMBEDS into its
    /// fresh result ([`crate::primitives::def::PrimitiveDef::embeds`]). The region
    /// walk's `Fresh` arm records a `result ⊇ arg` containment edge
    /// (`RegionInfo::containment_edges`) for each, so the ownership forest sees a value
    /// the fresh result keeps a reference to — a captured trait table `with-traits`
    /// embeds into an escaping value — and refuses to adopt it. Empty by default (the
    /// walk records no embed edges); only the real primitive classification
    /// (`PrimitiveClassification::new`) fills it.
    pub embeds: rustc_hash::FxHashMap<SymbolId, &'static [usize]>,
    /// Letrec-bound Bindings whose lambda bodies return immediates.
    /// Populated by the callee fixpoint pre-pass.
    pub user_immediates: FxHashSet<Binding>,
    /// SymbolIds of the value-RETAINING store intrinsics — the `Funnel` ops whose
    /// runtime body increfs the stored heap value (`%put`/`%put-*`/`%array-push`/
    /// set `%add`). `Funnel` alone is too broad: it also covers REMOVALS (`%del` —
    /// decrefs the value) and BYTE-COPY pushes (`%string-push`/`%bytes-push` —
    /// retain no region). Only at a retaining-store site is the stored value's RC
    /// raised, the invariant `regions::compensate` needs to place a per-arm decref
    /// that cannot over-free. Populated by `PrimitiveClassification::new`.
    pub retaining_store_funnels: FxHashSet<SymbolId>,
    /// The SymbolId of `fiber/new`, when the symbol table carries it — the
    /// transferred-returned-subtree cut (`regions::ownership::transfer`) must
    /// recognize a fiber-body producer structurally. `None` under the default
    /// empty classification, which disables that cut's fiber face.
    pub fiber_new: Option<SymbolId>,
    /// The SymbolId of `fiber/resume` — the fiber face's consumer site (a
    /// completing resume hands back the body's terminal value). `None` under
    /// the default empty classification.
    pub fiber_resume: Option<SymbolId>,
}
