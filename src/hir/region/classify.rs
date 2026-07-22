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
    /// SymbolIds of the BYTE-COPY store funnels (`%string-push`/`%string-push-mut`/
    /// `%bytes-push`) — `Funnel` ops that copy the pushed value's bytes rather than
    /// retaining its region. Neither retaining (no member incref) nor removing (no
    /// in-body decref). A dispatch wrapper's `val` param strands across arms exactly
    /// as a retaining store's does, but here the per-arm release is `val`'s ACTUAL
    /// last use (the byte-copy touched neither its incref nor its decref), so it is
    /// sound to compensate — the `%del` in-body-decref double-free hazard the
    /// compensation excludes does NOT apply. `regions::compensate` releases the
    /// stranded `val` per-arm from `funnel_bytecopy_value_sites`. Populated by
    /// `PrimitiveClassification::new`.
    pub bytecopy_store_funnels: FxHashSet<SymbolId>,
    /// SymbolIds of the moves-out natives whose result is a genuinely non-fresh
    /// PASS-THROUGH element removed from a container (`%pop`/`%pop-array*`): the
    /// native body escape-retains the moved-out element in place (before releasing
    /// the container), so in TAIL position the lowerer's extra ReturnValue retain
    /// double-counts and over-frees it (`region_pop_tail_moves_out_uaf`). The walk
    /// records these sites (`moves_out_release_sites`) so the lowerer suppresses
    /// that redundant retain. Restricted to the `PassThrough` subset of moves-out
    /// natives: a fresh grapheme (`@string` pop) / immediate byte is NOT
    /// escape-retained in body and NEEDS its tail retain, so it is excluded here.
    /// Populated by `PrimitiveClassification::new`.
    pub moves_out_passthrough: FxHashSet<SymbolId>,
    /// SymbolIds of ALL moves-out REMOVE natives (`%pop`/`%pop-string`/`%pop-bytes`),
    /// regardless of effect. A `pop` dispatch wrapper uses its container arg0 in every
    /// `(match (type-of coll) …)` arm but frees it in ONE, so the owned-param reference
    /// strands on every other arm — the F1b container strand `add`/`del` have. The walk
    /// records arg0 as a `funnel_container_sites` container so `regions::compensate`
    /// releases it per-arm. Distinct from `moves_out_passthrough` (the PassThrough
    /// subset, for the ELEMENT's tail-retain suppression): the CONTAINER strand affects
    /// the fresh-result (`Funnel`) arms too. Populated by `PrimitiveClassification::new`.
    pub moves_out: FxHashSet<SymbolId>,
    /// SymbolIds of the container element-READ natives (`first`/`rest`/`get`/`pop`
    /// and their `%`-op peers) — the ops whose result is a value read OUT of the
    /// container passed as arg0. Escape uses this to add a **read-result →
    /// container-contents** flow edge: a value stored into a container and then read
    /// back out and escaped must not be adopted into the container's Owned subtree
    /// (else the container's subtree drop frees a value that flows out — the
    /// container-read-escape face, pinned by `region_container_read_escape_uaf`).
    /// Populated by `PrimitiveClassification::new`.
    pub container_read_funnels: FxHashSet<SymbolId>,
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
