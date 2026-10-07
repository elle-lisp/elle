// audited: 2026-10-06
//! The transferred-returned-subtree cut: a producer's returned cycle, owned by the activation that consumes it.
//!
//! docs/impl/region/owner.md
//!
//! A producer builds an externally-unique subtree containing a reference cycle
//! and hands its root to a consumer. There are three faces. On the call face the
//! producer is a lambda whose root crosses the return frontier. On the fiber
//! face it is a fiber body whose terminal value a completing resume hands back.
//! On the inline face it is a binding's initializer, the shape a grafted call
//! leaves. Every region-rooted mode refuses the root, and the consumer's release
//! drops one reference while the cycle's interior back-edge holds another, so
//! per-region RC never frees it. The consuming activation's owner node reclaims
//! it: its completion release post-dominates every use of the root on either
//! side of the hand-off. Replacing the consumer's release with
//! `AdoptIntoActivation`, which consumes the whole count, lets the node's
//! set-drop reclaim root and members wholesale.
//!
//! Producer and consumer halves are admitted only together: the interior adopts
//! freeze member counts, so a consumer that could alias a member out of the
//! node's reclamation horizon refuses the whole producer.

use super::super::escape::captured_bindings;
use super::super::*;
use super::inputs::OwnershipInputs;
use rustc_hash::{FxHashMap, FxHashSet};

mod call;
mod candidates;
mod consumer;
mod cut;
mod fiber;
mod helpers;
mod inline;
mod summary;
mod useindex;

use candidates::*;
use cut::*;
use helpers::*;
use summary::*;
use useindex::*;

/// The transfer cut's output. `analyze::adopt` merges the interior owner edges
/// into the ordinary adopt maps, `RegionInfo::owned_adopt_edges` and
/// `capture_adopt_edges`. They take the same emission and, for capture members,
/// the same suppress ⊆ adopt contract. `result_regions` are the consumer regions
/// whose release the lowerer replaces with `AdoptIntoActivation`
/// (`RegionInfo::transfer_adopt_regions`).
pub(in crate::hir::region::infer) struct TransferAdopts {
    /// Emit-site HirId (a store site, or a funnel call site — the funnel store
    /// face, where the adopt is value-resolved and needs no store opcode) →
    /// interior `(member, owner)` adopts.
    pub store: HashMap<HirId, Vec<(Region, Region)>>,
    /// Closure-construction HirId → interior `(captured member, closure)` adopts.
    pub capture: HashMap<HirId, Vec<(Region, Region)>>,
    /// Consumer regions to release by `AdoptIntoActivation`: a call or resume
    /// site's result region, or an inline producer's root.
    pub result_regions: FxHashSet<Region>,
}

/// Compute the transfer cut over one unit, face by face. A root one face admits
/// is never admitted again.
pub(in crate::hir::region::infer) fn compute_transfer_adopts(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    arena: &BindingArena,
    call_class: &CallClassification,
    order: &HashMap<HirId, u32>,
) -> TransferAdopts {
    let cut = Cut::new(inputs, hir, info, escape, arena, call_class, order);
    let mut taken_roots: FxHashSet<Region> = FxHashSet::default();
    let mut admitted: Vec<(Summary, Vec<Region>)> = Vec::new();
    call::admit(&cut, hir, arena, &mut taken_roots, &mut admitted);
    fiber::admit(
        &cut,
        hir,
        arena,
        call_class,
        &mut taken_roots,
        &mut admitted,
    );
    inline::admit(&cut, hir, &mut taken_roots, &mut admitted);

    let mut out = TransferAdopts {
        store: HashMap::new(),
        capture: HashMap::new(),
        result_regions: FxHashSet::default(),
    };
    for (summary, results) in admitted {
        for (site, m, owner) in summary.store_edges {
            out.store.entry(site).or_default().push((m, owner));
        }
        for (site, m, owner) in summary.capture_edges {
            out.capture.entry(site).or_default().push((m, owner));
        }
        out.result_regions.extend(results);
    }
    out
}
