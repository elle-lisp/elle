//! Region-ownership test helpers (pair-store edges, shared seeds, owned
//! subtrees, adopt edges). Split from `helpers.rs` to keep each file < 500
//! lines. `super::ownership` resolves via the re-export in `mod.rs`.
use super::*;

/// Every `%pair` car/cdr store edge `(child, parent)` in the program — the
/// immutable-aggregate-store edges the merge seed considers. An edge is one such
/// store iff its site is a `Pair` intrinsic AND the edge target is the region
/// freshly allocated at that site (`alloc_region[site]`), which uniquely picks the
/// pair's own car/cdr stores out of `cross_region_refs` (push/put/set-cell/clique
/// edges target something other than the site's own allocation).
pub(super) fn pair_store_edges(hir: &Hir, info: &RegionInfo) -> Vec<(Region, Region)> {
    let mut pair_sites = Vec::new();
    find_all_pairs_helper(hir, &mut pair_sites);
    info.cross_region_refs
        .iter()
        .filter(|(site, _, dst)| {
            pair_sites.contains(site) && info.alloc_region.get(site) == Some(dst)
        })
        .map(|&(_, src, dst)| (src, dst))
        .collect()
}

/// Compile `source`, run region inference + escape (the same default
/// `CallClassification` the solver itself uses), and compute the Shared-seed set,
/// so the seed input is exactly what `analyze_regions` saw.
pub(super) fn shared_seeds(source: &str) -> (Hir, RegionInfo, rustc_hash::FxHashSet<Region>) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);
    let escape = crate::hir::analyze_escape(&hir, &arena, &CallClassification::default());
    let seeds = super::ownership::compute_shared_seeds(&info, &escape);
    (hir, info, seeds)
}

/// Like `shared_seeds`, but threads the REAL primitive `CallClassification` (the
/// declared `RegionEffect`s) into both region inference and escape. Required for the
/// fiber **send** facet: its seed fires only when the solver resolves `chan/send` to
/// its declared `Sends` effect — the default empty classification `shared_seeds` uses
/// treats every call as an opaque user fn (no effect), so a send would not seed.
pub(super) fn shared_seeds_with_effects(
    source: &str,
) -> (Hir, RegionInfo, rustc_hash::FxHashSet<Region>) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let seeds = super::ownership::compute_shared_seeds(&info, &escape);
    (hir, info, seeds)
}

/// The region of the single `%pair` site in `hir` (the seed shapes each have one).
pub(super) fn sole_pair_region(hir: &Hir, info: &RegionInfo) -> Region {
    let mut pairs = Vec::new();
    find_all_pairs_helper(hir, &mut pairs);
    assert_eq!(
        pairs.len(),
        1,
        "seed test shape must have exactly one %pair"
    );
    *info
        .alloc_region
        .get(&pairs[0])
        .expect("the %pair has an alloc region")
}

/// Compile, run inference + escape (the default `CallClassification`, exactly as
/// `analyze_regions` does), and compute the Owned-subtree map.
pub(super) fn owned_subtrees(
    source: &str,
) -> (
    Hir,
    RegionInfo,
    rustc_hash::FxHashMap<Region, rustc_hash::FxHashSet<Region>>,
) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let info = analyze_regions(&hir, &arena);
    let escape = crate::hir::analyze_escape(&hir, &arena, &CallClassification::default());
    let owned = super::ownership::compute_owned_subtrees(&hir, &info, &escape, &arena);
    (hir, info, owned)
}

/// Like `owned_subtrees`, threading the REAL primitive `CallClassification` so declared
/// `RegionEffect`s reach inference — required for shapes whose ownability depends on a
/// declared effect: `@array`/`array` resolving to `Fresh` (a freshly-built container is
/// then an Owned candidate; the default empty effects would refuse it as an opaque
/// call-result), and the may-store-clique negative (`has?` resolving to its declared
/// `Mixed`, recording the hard clique that defeats external uniqueness —
/// `owned_subtree_refuses_may_store_clique`).
pub(super) fn owned_subtrees_with_effects(
    source: &str,
) -> (
    Hir,
    RegionInfo,
    rustc_hash::FxHashMap<Region, rustc_hash::FxHashSet<Region>>,
) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let owned = super::ownership::compute_owned_subtrees(&hir, &info, &escape, &arena);
    (hir, info, owned)
}

/// Is `r` in any Owned subtree — a root key or a member of some subtree's region set?
pub(super) fn in_some_owned_subtree(
    owned: &rustc_hash::FxHashMap<Region, rustc_hash::FxHashSet<Region>>,
    r: Region,
) -> bool {
    owned.values().any(|s| s.contains(&r))
}

/// Like `owned_subtrees_with_effects`, but compiles on the **checked-on**
/// (native-Call) production path, where `%`-intrinsics route through their
/// `NativeFn` primitives — so `%array-push`/`%put` is an opaque `Funnel` CALL that
/// records NO `cross_region_refs` edge. This is the path the alloc-type containment
/// recovery (`RegionInfo::containment_edges`) exists for. The thread-local override
/// keeps the flip scoped to this one compile (the global config is write-once).
pub(super) fn owned_subtrees_checked_on(
    source: &str,
) -> (
    Hir,
    RegionInfo,
    rustc_hash::FxHashMap<Region, rustc_hash::FxHashSet<Region>>,
) {
    let mut symbols = SymbolTable::new();
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let (hir, arena) = {
        let _guard = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
        let (hir, arena, _) = compile_fhir(source, &mut symbols);
        (hir, arena)
    };
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let owned = super::ownership::compute_owned_subtrees(&hir, &info, &escape, &arena);
    (hir, info, owned)
}

/// Compile, run inference + escape under the REAL primitive classification (so
/// `@array`/`array` resolve to `Fresh` and `%array-push` records its `cross_region_refs`
/// containment edge on the default checked-off test path), and compute the adopt-edge
/// map `compute_adopt_edges` hands the lowerer. Mirrors `owned_subtrees_with_effects`,
/// threading the `compute_order` index `compute_adopt_edges` needs for the lifetime
/// obligation.
pub(super) fn adopt_edges(source: &str) -> (Hir, RegionInfo, super::ownership::AdoptEdges) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let order = compute_order(&hir);
    let edges = super::ownership::compute_adopt_edges(&hir, &info, &escape, &arena, &order);
    (hir, info, edges)
}

/// Like [`adopt_edges`], but compiled on the **checked-on** (native-Call) production
/// path, where `%array-push`/`%put` is an opaque `Funnel` call recording NO
/// `cross_region_refs` edge — the containment reaches the adopt walk only as
/// site-keyed funnel-recovered `containment_edges`, and the emitted adopt is keyed at
/// the funnel call site (the funnel face of the store-keyed adopt;
/// region-model.md § "The funnel adopt — the checked-on store face").
pub(super) fn adopt_edges_checked_on(
    source: &str,
) -> (Hir, RegionInfo, super::ownership::AdoptEdges) {
    let mut symbols = SymbolTable::new();
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let (hir, arena) = {
        let _guard = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
        let (hir, arena, _) = compile_fhir(source, &mut symbols);
        (hir, arena)
    };
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let order = compute_order(&hir);
    let edges = super::ownership::compute_adopt_edges(&hir, &info, &escape, &arena, &order);
    (hir, info, edges)
}

/// Compile under the REAL primitive classification and compute the co-owned-cycle
/// groups (`compute_owned_region_groups`): drop-site HirId → member regions. Mirrors
/// [`adopt_edges`], threading the `compute_order` index the group walk needs for the
/// drop-site post-dominance check and the deterministic member emit-order.
pub(super) fn owned_region_groups(source: &str) -> (Hir, RegionInfo, HashMap<HirId, Vec<Region>>) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let order = compute_order(&hir);
    let groups =
        super::ownership::compute_owned_region_groups(&hir, &info, &escape, &arena, &order);
    (hir, info, groups)
}

/// Compile under the REAL primitive classification and run the whole inference
/// with `--region-ownership` scoped ON, returning the populated `RegionInfo` —
/// the view the lowerer consumes (`activation_adopt_sites`,
/// `suppressed_decref_regions`, the adopt/group maps together). The
/// activation-adopt pins read THIS (not a direct `compute_*` call) so they are
/// red until `analyze_regions_with` actually wires the cut.
pub(super) fn analyze_flag_on(source: &str) -> (Hir, RegionInfo) {
    use crate::config::region_ownership_override::{RegionOwnership, ScopedRegionOwnership};
    let _g = ScopedRegionOwnership::new(RegionOwnership::On);
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let info = analyze_regions_with(&hir, &arena, pc.call_classification);
    (hir, info)
}

/// Like [`analyze_flag_on`] but compiled on the **checked-on** (native-Call)
/// production path, where a store is an opaque `Funnel` call recording NO
/// `cross_region_refs` edge — the containment reaches the inference only through
/// the funnel-recovered `RegionInfo::containment_edges`. The activation-adopt
/// cut must admit through that face too (its emit is value-resolved, needing no
/// store site).
pub(super) fn analyze_flag_on_checked_on(source: &str) -> (Hir, RegionInfo) {
    use crate::config::region_ownership_override::{RegionOwnership, ScopedRegionOwnership};
    let _g = ScopedRegionOwnership::new(RegionOwnership::On);
    let mut symbols = SymbolTable::new();
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let (hir, arena) = {
        let _guard = crate::config::test_override::ScopedCheckedIntrinsics::new(true);
        let (hir, arena, _) = compile_fhir(source, &mut symbols);
        (hir, arena)
    };
    let info = analyze_regions_with(&hir, &arena, pc.call_classification);
    (hir, info)
}

/// The container **root** (the region that is the TARGET of a non-hard containment
/// edge but never a SOURCE — a top container holds members; it is held by nothing) and
/// the **member** regions (the edge sources) of a discarded shared-container subtree.
pub(super) fn container_root_and_members(info: &RegionInfo) -> (Region, Vec<Region>) {
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(site, src, dst) in &info.cross_region_refs {
        if info.hard_edge_sites.contains(&site) {
            continue;
        }
        srcs.insert(src);
        dsts.insert(dst);
    }
    let roots: Vec<Region> = dsts.iter().copied().filter(|r| !srcs.contains(r)).collect();
    assert_eq!(
        roots.len(),
        1,
        "shape must have exactly one container root (a target-only region); got {:?}",
        roots
    );
    let members: Vec<Region> = srcs.into_iter().collect();
    (roots[0], members)
}

/// Which reload path would `lower_lambda_expr` use for the capture-adopt edge
/// `(member, closure)` — true for a LOCAL-SLOT reload (`LoadLocal`), false for an
/// env reload (`LoadCapture` — an upvalue or transitive capture; region-model.md
/// § "The capture adopt"). Slot-loaded iff the lambda whose `alloc_region` is
/// `closure` captures the binding holding `member` with `CaptureKind::Local` AND no
/// lambda lexically *enclosing* it also captures that binding. Both paths are
/// emittable; boundary pins use this to assert a shape genuinely exercises the
/// env-loaded (cross-activation) side.
pub(super) fn capture_is_local_slot_loaded(
    hir: &Hir,
    info: &RegionInfo,
    member: Region,
    closure: Region,
) -> bool {
    use crate::hir::CaptureKind;
    fn walk(
        h: &Hir,
        info: &RegionInfo,
        member: Region,
        closure: Region,
        enclosing: &mut Vec<rustc_hash::FxHashSet<crate::hir::Binding>>,
    ) -> Option<bool> {
        if let HirKind::Lambda { captures, .. } = &h.kind {
            if info.alloc_region.get(&h.id) == Some(&closure) {
                // The capturing closure: find its capture of the binding holding `member`.
                let cap = captures.iter().find(|c| {
                    info.binding_source_regions
                        .get(&c.binding)
                        .is_some_and(|rs| rs.contains(&member))
                });
                return Some(cap.is_some_and(|c| {
                    matches!(c.kind, CaptureKind::Local | CaptureKind::Recursive { .. })
                        && !enclosing.iter().any(|e| e.contains(&c.binding))
                }));
            }
            enclosing.push(captures.iter().map(|c| c.binding).collect());
            let mut res = None;
            h.for_each_child(|c| {
                if res.is_none() {
                    res = walk(c, info, member, closure, enclosing);
                }
            });
            enclosing.pop();
            res
        } else {
            let mut res = None;
            h.for_each_child(|c| {
                if res.is_none() {
                    res = walk(c, info, member, closure, enclosing);
                }
            });
            res
        }
    }
    walk(hir, info, member, closure, &mut Vec::new()).unwrap_or(false)
}
