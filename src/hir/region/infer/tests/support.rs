//! Region-ownership test helpers (pair-store edges, shared seeds, owned
//! subtrees, adopt edges). Split from `helpers.rs` to keep each file < 500
//! lines. `super::ownership` resolves via the re-export in `mod.rs`.
use super::*;

/// The ownership forest is unconditional, so `analyze_regions`/`analyze_regions_with`
/// run the ownership pass and, AFTER computing the adopt/group maps, suppress each
/// capture-adopted and activation-adopted member's own decref (adding them to
/// `suppressed_decref_regions`). A test helper that re-runs a `compute_*` ownership walk
/// directly on the returned `RegionInfo` would then see those members as `not_ownable`
/// (`ownership::inputs::not_ownable` reads `suppressed_decref_regions`) — a self-poisoning
/// the production single pass never hits, because suppression is applied only AFTER the
/// walks run internally. Undo exactly the ownership-added suppressions so a direct re-run
/// reproduces the pre-suppression view the internal pass computed against. The
/// reassign-gate suppressions (the other contributor to `suppressed_decref_regions`) are
/// left intact — `not_ownable` must still see them.
fn restore_pre_ownership_view(info: &mut RegionInfo) {
    let ownership_suppressed: Vec<Region> = info
        .capture_adopt_edges
        .values()
        .flatten()
        .map(|&(member, _closure)| member)
        .chain(info.activation_adopt_sites.values().flatten().copied())
        .collect();
    for r in ownership_suppressed {
        info.suppressed_decref_regions.remove(&r);
    }
}

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
    let mut info = analyze_regions(&hir, &arena);
    restore_pre_ownership_view(&mut info);
    let escape = crate::hir::analyze_escape(&hir, &arena, &CallClassification::default());
    let inputs = super::ownership::ownership_inputs(&hir, &info, &escape, &arena);
    let owned = super::ownership::compute_owned_subtrees(&inputs, &info);
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
    let mut info = analyze_regions_with(&hir, &arena, cc.clone());
    restore_pre_ownership_view(&mut info);
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let inputs = super::ownership::ownership_inputs(&hir, &info, &escape, &arena);
    let owned = super::ownership::compute_owned_subtrees(&inputs, &info);
    (hir, info, owned)
}

/// Is `r` in any Owned subtree — a root key or a member of some subtree's region set?
pub(super) fn in_some_owned_subtree(
    owned: &rustc_hash::FxHashMap<Region, rustc_hash::FxHashSet<Region>>,
    r: Region,
) -> bool {
    owned.values().any(|s| s.contains(&r))
}

/// Compile, run inference + escape under the REAL primitive classification (so
/// `@array`/`array` resolve to `Fresh`), and compute the adopt-edge map
/// `compute_adopt_edges` hands the lowerer. A `%`-store (`%array-push`/`%put`)
/// is an opaque `Funnel` native call recording NO `cross_region_refs` edge —
/// its containment reaches the adopt walk as site-keyed funnel-recovered
/// `containment_edges`, and the emitted adopt is keyed at the funnel call
/// site (region/adopt.md § The funnel adopt). Mirrors
/// `owned_subtrees_with_effects`, threading the `compute_order` index
/// `compute_adopt_edges` needs for the lifetime obligation.
pub(super) fn adopt_edges(source: &str) -> (Hir, RegionInfo, super::ownership::AdoptEdges) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let mut info = analyze_regions_with(&hir, &arena, cc.clone());
    restore_pre_ownership_view(&mut info);
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let order = compute_order(&hir);
    let inputs = super::ownership::ownership_inputs(&hir, &info, &escape, &arena);
    let edges = super::ownership::compute_adopt_edges(&inputs, &hir, &info, &arena, &order);
    (hir, info, edges)
}

/// Compile under the REAL primitive classification and re-derive the capture
/// containment edges `(lambda_id, captured_region, closure_region)` the ownership
/// walks consume ([`capture_containment_edges`](super::ownership::capture_containment_edges)).
/// Returns the arena alongside so a test can read a captured binding's
/// `is_restorable_capture_cell` classification and the info's `single_cell_region_of`.
/// These edges read only the WALK's output (`binding_source_regions`, `alloc_region`,
/// `begin_cell_regions`), untouched by the ownership-pass suppressions, so no
/// pre-ownership-view restore is needed.
pub(super) fn capture_edges(
    source: &str,
) -> (Hir, BindingArena, RegionInfo, Vec<(HirId, Region, Region)>) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let cc = pc.call_classification;
    let info = analyze_regions_with(&hir, &arena, cc.clone());
    let edges = super::ownership::capture_containment_edges(&hir, &info, &arena);
    (hir, arena, info, edges)
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
    let mut info = analyze_regions_with(&hir, &arena, cc.clone());
    restore_pre_ownership_view(&mut info);
    let escape = crate::hir::analyze_escape(&hir, &arena, &cc);
    let order = compute_order(&hir);
    let inputs = super::ownership::ownership_inputs(&hir, &info, &escape, &arena);
    let groups = super::ownership::compute_owned_region_groups(&inputs, &hir, &info, &order);
    (hir, info, groups)
}

/// Compile under the REAL primitive classification and run the whole (ownership)
/// inference, returning the populated `RegionInfo` — the view the lowerer consumes
/// (`activation_adopt_sites`, `suppressed_decref_regions`, the adopt/group maps
/// together). The activation-adopt pins read THIS (not a direct `compute_*` call)
/// so they exercise the same wiring `analyze_regions_with` hands the lowerer.
pub(super) fn analyze_full(source: &str) -> (Hir, RegionInfo) {
    let mut symbols = SymbolTable::new();
    let (hir, arena, _) = compile_fhir(source, &mut symbols);
    let meta = crate::primitives::build_primitive_meta(&mut symbols);
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(&symbols, &meta);
    let info = analyze_regions_with(&hir, &arena, pc.call_classification);
    (hir, info)
}

/// The container **root** (the region that is the TARGET of a containment edge but
/// never a SOURCE — a top container holds members; it is held by nothing) and the
/// **member** regions (the edge sources) of a discarded shared-container subtree.
/// Reads the funnel-recovered `containment_edges`: a `%`-store is an opaque `Funnel`
/// native call recording NO `cross_region_refs` edge, so containment reaches the
/// ownership walks only through this site-keyed map.
pub(super) fn container_root_and_members(info: &RegionInfo) -> (Region, Vec<Region>) {
    let mut srcs = rustc_hash::FxHashSet::default();
    let mut dsts = rustc_hash::FxHashSet::default();
    for &(_site, src, dst) in &info.containment_edges {
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
/// env reload (`LoadCapture` — an upvalue or transitive capture; region/adopt.md
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
