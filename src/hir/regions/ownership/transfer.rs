//! The transferred-returned-subtree cut: a callee-built, externally-unique
//! subtree containing a reference cycle, handed to its consumer across the
//! return (or fiber-terminal) frontier and owned by the **consuming
//! activation** (docs/impl/region/owner.md § "Owner nodes" — "The transferred
//! returned subtree").
//!
//! Inside the producer the root crosses the return frontier, so every
//! region-rooted mode refuses (a Shared seed poisons the subtree and group
//! walks); in the consumer the root is an opaque call-result whose
//! `DecrefValueRegion` releases one reference while the cycle's interior
//! back-edge holds another — the cycle survives every release and leaks per
//! call. The consuming activation's owner node reclaims it: its completion
//! release post-dominates every use of the result on either side of the
//! frontier, so replacing the consumer's release with `AdoptIntoActivation`
//! (consuming the whole count, stuck back-edge reference included) lets the
//! node's set-drop reclaim root + members wholesale.

use super::super::escape::captured_bindings;
use super::super::*;
use super::inputs::OwnershipInputs;
use rustc_hash::{FxHashMap, FxHashSet};

mod candidates;
mod helpers;
mod summary;
mod useindex;

use candidates::*;
use helpers::*;
use summary::*;
use useindex::*;

/// The transfer cut's output, computed by the ownership pass in `analyze_regions_with`.
/// The interior owner edges are merged into the ordinary adopt maps
/// (`RegionInfo::owned_adopt_edges` / `capture_adopt_edges` — same emission,
/// same suppress ⊆ adopt contract for capture members); `result_regions` are
/// the consumer-site call-result regions whose release the lowerer replaces
/// with `AdoptIntoActivation` (`RegionInfo::transfer_adopt_regions`).
pub(in crate::hir::regions) struct TransferAdopts {
    /// Emit-site HirId (a store site, or a funnel call site — the funnel store
    /// face, where the adopt is value-resolved and needs no store opcode) →
    /// interior `(member, owner)` adopts.
    pub store: HashMap<HirId, Vec<(Region, Region)>>,
    /// Closure-construction HirId → interior `(captured member, closure)` adopts.
    pub capture: HashMap<HirId, Vec<(Region, Region)>>,
    /// Consumer-site call-result regions to release by `AdoptIntoActivation`.
    pub result_regions: FxHashSet<Region>,
}

/// Compute the transfer cut (docs/impl/region/owner.md § "Owner nodes" — "The
/// transferred returned subtree"). Producer and consumer halves are admitted
/// only together: the interior adopts freeze member counts, so a consumer that
/// could alias a member out of the node's reclamation horizon refuses the
/// whole callee — one inadmissible site refuses every site.
pub(in crate::hir::regions) fn compute_transfer_adopts(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    arena: &BindingArena,
    call_class: &CallClassification,
    order: &HashMap<HirId, u32>,
) -> TransferAdopts {
    let mut out = TransferAdopts {
        store: HashMap::new(),
        capture: HashMap::new(),
        result_regions: FxHashSet::default(),
    };
    let shared = inputs.shared();
    let capture_edges = inputs.capture_edges();
    let captured = captured_bindings(hir);
    let ix = UseIndex::build(hir, arena, call_class);
    let low = compute_subtree_low(hir, order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);

    // Region → allocation site (real allocations + prebound capture cells), as
    // in the group/activation walks — the structural key for the born-inside
    // and dies-inside gates.
    let mut region_alloc_hir: FxHashMap<Region, HirId> = FxHashMap::default();
    for (&hid, &reg) in &info.alloc_region {
        region_alloc_hir.insert(reg, hid);
    }
    for (&begin_id, cells) in &info.begin_cell_regions {
        for &(_b, reg) in cells {
            region_alloc_hir.insert(reg, begin_id);
        }
    }

    // The fiber-frontier-only seed halves (emit / send): the returned root may
    // cross the RETURN frontier — that is the shape — but an emitted/sent root
    // has an unbounded second consumer and refuses.
    let mut fiber_seeds: FxHashSet<Region> = FxHashSet::default();
    for (&b, regions) in &info.binding_source_regions {
        if escape.escapes_fiber(b) {
            fiber_seeds.extend(regions.iter().copied());
        }
    }
    for (&hid, &r) in &info.alloc_region {
        if escape.escapes_fiber_frontier(hid) {
            fiber_seeds.insert(r);
        }
    }

    // Regions already claimed by the merge forest (builder-idiom or
    // closure-cycle): never transfer members (the one-owner invariant).
    let is_merged = |r: Region| -> bool {
        info.merged_parent.contains_key(&r)
            || info.merged_parent.values().any(|&p| p == r)
            || info.closure_cycle_members.contains(&r)
    };
    // A region touched by any edge of any kind, in either role — the consumer
    // gate's "appears in no edge" test (hard may-stores included: a may-holder
    // may hold).
    let in_any_edge = |r: Region| -> bool {
        info.cross_region_refs
            .iter()
            .any(|&(_, s, d)| s == r || d == r)
            || capture_edges.iter().any(|&(_, s, d)| s == r || d == r)
            || info
                .containment_edges
                .iter()
                .any(|&(_, s, d)| s == r || d == r)
    };

    // ── The producer summary ────────────────────────────────────────────────
    // The returned subtree of lambda `l`, or `None` when any gate refuses.
    let summarize = |l: &Hir| -> Option<Summary> {
        let HirKind::Lambda { body, .. } = &l.kind else {
            return None;
        };
        let l_low = low.get(&l.id).copied().unwrap_or(0);
        let l_hi = ord(l.id);
        let inside = |id: HirId| -> bool {
            let o = ord(id);
            l_low <= o && o <= l_hi
        };
        // The tail must resolve, through the structural wrappers, to a single
        // binding read with exactly one source region — the root. A branch
        // mix, a bare call, or an aggregate tail refuses (no single region to
        // hand to the consumer's value-resolved adopt).
        fn tail_root(h: &Hir, info: &RegionInfo) -> Option<Region> {
            match &h.kind {
                HirKind::Return { value } => tail_root(value, info),
                HirKind::Begin(es) => tail_root(es.last()?, info),
                HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => tail_root(body, info),
                HirKind::DerefCell { cell } => tail_root(cell, info),
                HirKind::Var(b) => match info.binding_source_regions.get(b)?.as_slice() {
                    [r] => Some(*r),
                    _ => None,
                },
                _ => None,
            }
        }
        let root = tail_root(body, info)?;
        // The root is born in the producer, is fresh (a Fresh call-result or a
        // live local allocation — an opaque result could be a borrow), crosses
        // no fiber frontier, and carries no dynamic-lifetime class. It DOES
        // cross the return frontier — that is the transfer.
        if !inside(region_alloc_hir.get(&root).copied()?) {
            return None;
        }
        let root_fresh = info.fresh_result_regions.contains(&root)
            || (info.live_regions.contains(&root) && !info.call_result_regions.contains(&root));
        if !root_fresh
            || fiber_seeds.contains(&root)
            || info.cell_release_regions.contains(&root)
            || info.suppressed_decref_regions.contains(&root)
            || info.mutated_binding_value_regions.contains(&root)
            || is_merged(root)
        {
            return None;
        }
        // Members: born and last-used inside the producer, no frontier, no
        // dynamic class, sole-held, unclaimed.
        let subtree = inputs.reach(root);
        for &m in &subtree {
            if m == root {
                continue;
            }
            if inputs.not_ownable(info, m) || !inputs.sole_held(m) || is_merged(m) {
                return None;
            }
            let &alloc = region_alloc_hir.get(&m)?;
            let dp = info.region_data.get(&m).map(|d| d.decref_point)?;
            if !inside(alloc) || !inside(dp) {
                return None;
            }
        }
        // External uniqueness: nothing inside references out (the return
        // itself records no edge).
        if inputs.outside_ref_in(&subtree) {
            return None;
        }
        // The subtree must contain an interior cycle: an acyclic returned
        // subtree reclaims promptly by the RC cascade today, and adopting it
        // would only trade that promptness for the activation bound.
        let has_cycle = subtree.iter().any(|&m| {
            inputs
                .reach(m)
                .iter()
                .any(|&m2| m2 != m && inputs.reach(m2).contains(&m))
        });
        if !has_cycle {
            return None;
        }
        // Interior owner edges, exactly as the store/capture adopt assigns
        // them — plus the funnel store face: a containment edge is emittable
        // at the funnel call site recording the stored member (the adopt is
        // value-resolved there and needs no store opcode).
        let interior_store: Vec<(HirId, Region, Region)> = info
            .cross_region_refs
            .iter()
            .copied()
            .filter(|(site, s, d)| {
                !info.hard_edge_sites.contains(site) && subtree.contains(s) && subtree.contains(d)
            })
            .collect();
        let interior_capture: Vec<(HirId, Region, Region)> = capture_edges
            .iter()
            .copied()
            .filter(|(_l, s, d)| subtree.contains(s) && subtree.contains(d))
            .collect();
        // The emit site is the containment edge's own funnel call, inside the
        // producer — gated to a retaining-store site recording `s` (the stored
        // value; a `%del`/key read never retains, so it stays unemittable).
        let interior_funnel: Vec<(HirId, Region, Region)> = info
            .containment_edges
            .iter()
            .copied()
            .filter(|&(site, s, d)| {
                subtree.contains(&s)
                    && subtree.contains(&d)
                    && inside(site)
                    && info
                        .funnel_store_sites
                        .get(&site)
                        .is_some_and(|stored| stored.contains(&s))
            })
            .collect();
        // Single owner per member: the root when a direct edge exists, else
        // the unique interior container; the chosen edge must be emittable.
        let mut containers_of: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
        for &(_, s, d) in interior_store
            .iter()
            .chain(interior_capture.iter())
            .chain(interior_funnel.iter())
        {
            containers_of.entry(s).or_default().insert(d);
        }
        let mut store_edges: Vec<(HirId, Region, Region)> = Vec::new();
        let mut cap_edges: Vec<(HirId, Region, Region)> = Vec::new();
        for &m in &subtree {
            if m == root {
                continue;
            }
            let owner = match containers_of.get(&m) {
                Some(cs) if cs.contains(&root) => root,
                Some(cs) if cs.len() == 1 => *cs.iter().next().unwrap(),
                _ => return None,
            };
            // Prefer a store/funnel site (the edge's own site), else the
            // capture's closure-construction site.
            if let Some(&(site, s, d)) = interior_store
                .iter()
                .chain(interior_funnel.iter())
                .find(|&&(_, s, d)| s == m && d == owner)
            {
                store_edges.push((site, s, d));
            } else if let Some(&(lambda, s, d)) = interior_capture
                .iter()
                .find(|&&(_, s, d)| s == m && d == owner)
            {
                cap_edges.push((lambda, s, d));
            } else {
                return None;
            }
        }
        Some(Summary {
            root,
            store_edges,
            capture_edges: cap_edges,
        })
    };

    // ── The consumer gate ───────────────────────────────────────────────────
    // A call site's result region is adoptable iff it crosses no frontier,
    // appears in no edge, belongs to no dynamic class, and is discard-shaped:
    // every binding the result flows into is read only through the
    // Immediate-native allowance (extraction through a pass-through native
    // records no edge, so the shape gate is what keeps an uncounted member
    // borrow from escaping the node's reclamation horizon). The result flow is
    // followed BOTH through the site's own placeholder region and through the
    // producer's ROOT region: an inlined producer's result flows into the
    // caller's bindings as the root region directly (the walk's
    // `try_inline_call` returns the body's tail regions). A holder whose every
    // recorded init sits inside the producer is the producer's own binding
    // (its interior reads precede the return); a holder bound outside is a
    // consumer-side read and must pass the allowance.
    let consumer_result = |site: HirId, root: Region, l_low: u32, l_hi: u32| -> Option<Region> {
        let &r = info.alloc_region.get(&site)?;
        if shared.contains(&r)
            || in_any_edge(r)
            || info.cell_release_regions.contains(&r)
            || info.suppressed_decref_regions.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || is_merged(r)
        {
            return None;
        }
        let reads_pass = |b: Binding| -> bool {
            ix.uses
                .get(&b)
                .is_none_or(|uses| uses.iter().all(|u| matches!(u, UseForm::ImmediateArg)))
        };
        // The bindings bound directly (or through the ANF wrapper) to this
        // call's value.
        if let Some(holders) = ix.bound_to.get(&site) {
            for &b in holders {
                if !reads_pass(b) {
                    return None;
                }
            }
        }
        // Every binding the result-flow regions reach, outside the producer.
        for (b, regions) in &info.binding_source_regions {
            if !(regions.contains(&r) || regions.contains(&root)) {
                continue;
            }
            // Producer-internal holder: every recorded init sits inside the
            // producer's own subtree interval (its reads precede the return).
            let inside_producer = ix.inits.get(b).is_some_and(|inits| {
                inits.iter().all(|&i| {
                    // SAFETY: init pointers index the HIR tree, which outlives
                    // this call.
                    let o = ord(unsafe { &*i }.id);
                    l_low <= o && o <= l_hi
                })
            });
            if inside_producer {
                continue;
            }
            if !reads_pass(*b) {
                return None;
            }
        }
        Some(r)
    };

    let mut admitted: Vec<(Summary, Vec<Region>)> = Vec::new();
    let mut taken_roots: FxHashSet<Region> = FxHashSet::default();

    // ── The call face ───────────────────────────────────────────────────────
    // Producer candidates in structural walk order: bindings with exactly one
    // Lambda init, used ONLY as a callee, every call site consumer-admitted.
    {
        let mut candidates: Vec<(Binding, *const Hir)> = Vec::new();
        collect_producers(hir, arena, &ix, &mut candidates);

        for (f, l) in candidates {
            // SAFETY: `l` points into the HIR tree, which outlives this call.
            let l = unsafe { &*l };
            // Every occurrence of `f` must be a callee position — any other
            // use (an alias, a HOF hand-off, a store) is an unknown consumer.
            let Some(uses) = ix.uses.get(&f) else {
                continue;
            };
            let sites: Vec<HirId> = uses
                .iter()
                .filter_map(|u| match u {
                    UseForm::Callee(site) => Some(*site),
                    _ => None,
                })
                .collect();
            if sites.is_empty() || sites.len() != uses.len() {
                continue;
            }
            let Some(summary) = summarize(l) else {
                continue;
            };
            if taken_roots.contains(&summary.root) {
                continue;
            }
            let (l_low, l_hi) = (low.get(&l.id).copied().unwrap_or(0), ord(l.id));
            let Some(results) = sites
                .iter()
                .map(|&s| consumer_result(s, summary.root, l_low, l_hi))
                .collect::<Option<Vec<Region>>>()
            else {
                continue;
            };
            taken_roots.insert(summary.root);
            admitted.push((summary, results));
        }
    }

    // ── The fiber face ──────────────────────────────────────────────────────
    // A fiber whose body's terminal value is a summarized subtree: every
    // completing resume hands it back; every other outcome is a fresh error
    // struct or an immediate, each safely adoptable (and a re-delivered
    // masked-error payload is absorbed by the channel's idempotence, bounded
    // to one activation by the same-function gate).
    {
        let mut fibers: Vec<FiberCand> = Vec::new();
        collect_fibers(hir, &ix, arena, call_class, &mut fibers);

        for (f2, new_site, l, body_binding) in fibers {
            // SAFETY: `l` points into the HIR tree, which outlives this call.
            let l = unsafe { &*l };
            let bi = arena.get(f2);
            if !bi.is_immutable || bi.is_mutated || captured.contains(&f2) {
                continue;
            }
            // The body must be unable to deliver a non-terminal value: no
            // yield / io / debug / wait bits and not polymorphic. Errors are
            // fine — an error delivery is a fresh struct, and a restarted
            // re-delivery of the same payload lands in the same activation,
            // where the channel's idempotence absorbs it.
            let HirKind::Lambda {
                inferred_signals, ..
            } = &l.kind
            else {
                continue;
            };
            let suspending = crate::signals::SIG_YIELD
                .union(crate::signals::SIG_IO)
                .union(crate::signals::SIG_DEBUG)
                .union(crate::signals::SIG_WAIT);
            if inferred_signals.propagates != 0 || inferred_signals.bits.intersects(suspending) {
                continue;
            }
            // When the body lambda came through a binding, that binding's
            // every use must be a fiber-body position — a body ALSO called
            // directly (or handed anywhere else) has un-gated consumers.
            if let Some(fb) = body_binding {
                let all_fiber_arg = ix
                    .uses
                    .get(&fb)
                    .is_some_and(|us| us.iter().all(|u| matches!(u, UseForm::FiberNewArg0(_))));
                if !all_fiber_arg {
                    continue;
                }
            }
            // The fiber's own region: fresh, unaliased, edge-free, held only
            // by this binding.
            let Some(&rf2) = info.alloc_region.get(&new_site) else {
                continue;
            };
            if shared.contains(&rf2) || in_any_edge(rf2) || !inputs.sole_held(rf2) {
                continue;
            }
            if info.binding_source_regions.get(&f2).map(|v| v.as_slice()) != Some([rf2].as_slice())
            {
                continue;
            }
            // Every use of f2 is a resume (a gated consumer site) or an
            // Immediate-native read, all in the SAME function body as the
            // binding — each activation then drives its own private fiber, so
            // no delivery can outlive the adopting activation.
            let Some(uses) = ix.uses.get(&f2) else {
                continue;
            };
            let new_encl = ix.enclosing.get(&new_site).copied().unwrap_or(None);
            let mut resume_sites: Vec<HirId> = Vec::new();
            let mut ok = true;
            for u in uses {
                match u {
                    UseForm::ResumeArg0(site) => {
                        if ix.enclosing.get(site).copied().unwrap_or(None) != new_encl {
                            ok = false;
                            break;
                        }
                        resume_sites.push(*site);
                    }
                    UseForm::ImmediateArg => {}
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if !ok || resume_sites.is_empty() {
                continue;
            }
            let Some(summary) = summarize(l) else {
                continue;
            };
            if taken_roots.contains(&summary.root) {
                continue;
            }
            let (l_low, l_hi) = (low.get(&l.id).copied().unwrap_or(0), ord(l.id));
            let Some(results) = resume_sites
                .iter()
                .map(|&s| consumer_result(s, summary.root, l_low, l_hi))
                .collect::<Option<Vec<Region>>>()
            else {
                continue;
            };
            taken_roots.insert(summary.root);
            admitted.push((summary, results));
        }
    }

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
