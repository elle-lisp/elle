// audited: 2026-10-21
//! Each Owned subtree's adopt edges: one owner per member, keyed by emit site, under the root's lifetime obligation.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/ownership.md

use super::super::postdom::{EmitMode, PostDom};
use super::super::*;
use super::aliases::AliasTables;
use super::inputs::OwnershipInputs;
use super::subtree::compute_owned_subtrees;
use rustc_hash::{FxHashMap, FxHashSet};

/// The adopt-edge maps the ownership forest's emit consumes, by emit site: the
/// output of `compute_adopt_edges` (docs/impl/region/ownership.md). The ownership
/// pass in `analyze_regions_with` computes them.
pub(in crate::hir::region::infer) struct AdoptEdges {
    /// `cross_region_refs` **store-site** HirId → `(member, owner)` adopts the lowerer
    /// emits in `emit_increfs_for` in place of the interior `IncrefRegion`
    /// (`RegionInfo::owned_adopt_edges`).
    pub store: HashMap<HirId, Vec<(Region, Region)>>,
    /// **Lambda** (closure-construction) HirId → `(captured, closure)` adopts the
    /// lowerer emits in `lower_lambda_expr` in place of the capture `IncrefRegion`
    /// (`RegionInfo::capture_adopt_edges`). Capture records no `cross_region_refs`
    /// store site, so the closure keys its adopt rather than a store node.
    /// `lower_lambda_expr` tells the member kinds apart by the captured binding. A
    /// **cell** region member (a re-pointed `closure ⊇ cell` edge) emits as
    /// `AdoptCellRegion`, on its own region rather than the unwrapped content. A
    /// by-value member emits as `AdoptRegion`.
    pub capture: HashMap<HirId, Vec<(Region, Region)>>,
    /// The bindings whose compiled capture cell adopts its stored **content** (a
    /// `cell ⊇ content` edge, `RegionInfo::cell_content_adopt_bindings`). At each such
    /// binding's cell-store site the lowerer emits `AdoptCellRegion(cell, content)`,
    /// linking the content's runtime region into the cell's own region (never unwrapped).
    /// Only immutable letrec cells reach here; a re-storable cell's content is refused
    /// (docs/impl/region/adopt.md). The holding closure capture-adopts the cell
    /// itself through `capture` above, so the whole `closure ⊇ cell ⊇ content` chain
    /// frees as one subtree.
    pub cell_content: Vec<Binding>,
}

/// The interior containment edges to emit as `AdoptRegion`, split by emit site:
/// the ownership forest's emit input. Wraps [`compute_owned_subtrees`] with the
/// **lifetime obligation** the external-uniqueness walk does not itself enforce, then
/// assigns each non-root member a single owner and projects its owner-edge to the
/// appropriate emit site.
///
/// Each emitted `(site → [(member, owner), …])` says: at `site`, link `member`'s
/// runtime region into `owner`'s Owned subtree (`AdoptRegion(owner, member)`)
/// instead of the interior edge's `IncrefRegion`. Each non-root member is adopted
/// by its **actual parent**. That is the root when a direct `member → root` edge
/// exists (a flat star, the common case). Otherwise it is the single interior
/// container that holds it: in `root ⊇ a ⊇ b`, `a` adopts `b` and the root adopts
/// `a`.
///
/// The owner graph is a forest, one owner per member. The root's single decref
/// drops the whole component recursively (`free_runtime_region_pages` walks
/// `owned_children`; `regionstore::tests::subtree_drop_is_recursive` pins it). An
/// interior member↔member cycle edge that is no member's chosen owner-edge
/// carries no adopt and needs none, because it reclaims with the subtree drop.
///
/// **Three containment-edge kinds, three emit sites.**
///
/// - A `cross_region_refs` store edge (`%pair`/`%array-push`/`%put`) carries its
///   store-site HirId. Its adopt rides `AdoptEdges::store`, which
///   `emit_increfs_for` consumes.
/// - A **capture** edge (`closure ⊇ captured`) has no store site, because a
///   capture records no `cross_region_refs` edge. Its adopt rides
///   `AdoptEdges::capture` keyed by the closure's construction HirId, consumed at
///   `MakeClosure` in `lower_lambda_expr`.
/// - A **funnel-recovered** containment edge (`RegionInfo::containment_edges`)
///   comes from a storing op that lowers as an opaque `Funnel` native call and
///   records no `cross_region_refs` edge. It carries its funnel **call-site**
///   HirId and rides `AdoptEdges::store` keyed there. The value-resolved adopt
///   reloads both endpoints from their binding slots, so it needs no store opcode,
///   the same face the activation and transfer cuts carry. A funnel edge is
///   emittable only at a **retaining-store** site recording the member as its
///   stored value (`funnel_store_sites`). A `%del` or key read retains nothing and
///   stays unemittable.
///
/// All three feed owner assignment. So a local Owned closure adopts a value it
/// captures, which is the per-call closure↔capture knot the forest exists to
/// reclaim. A funnel-built subtree reclaims exactly like a constructor-built one.
///
/// Three filters beyond the external-uniqueness walk. The lifetime obligation
/// reads the owner assignment, so the code checks it after the owner filter:
///
/// 1. **Lifetime obligation**, owner-aware, through the shared
///    `postdom::drop_post_dominates` in `EmitMode::Adopt`. The root's
///    `lifetime_point` must **structurally post-dominate** every member's relevant
///    last use over the scope tree (docs/impl/region/adopt.md). Otherwise the
///    root's single decref drops a region still derefed afterward, a UAF. A
///    numeric `ord` compare is no substitute, because across a branch arm or loop
///    back-edge order is not a post-dominance proxy.
///
///    Which last use bounds a member depends on whether its own decref is
///    SUPPRESSED. A **capture-adopted** member's own decref is suppressed, because
///    only the subtree drop frees it. So its over-extended release point is
///    harmless, and the TIGHT last use (`binding_last_use`) admits the safe capture
///    cut. A **store-adopted** member keeps its own `DecrefValueRegion` at its
///    structural release point. A Fresh call-result releases value-based, the
///    decref reading the region through `result_region_of`. So that structural
///    point MUST bound it. The tight value would free a captured-AND-store-adopted
///    member (the m↔c capture-back-edge cycle) before its own trailing decref
///    derefs the freed page.
///
///    A subtree that fails is dropped here, and no region root owns it. The
///    activation cut (`compute_activation_adopts`, owner = activation) then claims
///    the m↔c SCC itself, and only a shape that cut also refuses stays Shared. The
///    check is against the root only, because every member at any depth frees at
///    the root's drop.
///
///    The obligation covers derefs the members' own points do not name, the
///    aliases. A native `get`/`first`/`rest` can read a value back OUT of the
///    subtree (`RegionInfo::counted_read_aliases`). Its region is a fresh
///    call-result placeholder related to no member. Its Rule 5 pass-through
///    retain makes the read safe under RC, and goes inert once adoption freezes the
///    member. So the root's drop must post-dominate the ALIAS's release too, or
///    the subtree refuses to Shared, where the retain is live again. An opaque
///    CALL over a member (`RegionInfo::opaque_result_aliases`) may return a member
///    or the container, so it joins the same alias set. A release routed through a
///    compiled capture cell (`RegionInfo::cell_route_regions`) loads the cell and
///    unwraps it, so the routed region joins the set keyed by the cell.
/// 2. **No merge overlap.** A subtree touching any builder-idiom MERGE
///    participant is skipped. MERGE collapses those to one region, so adopting
///    them too would link a region that no longer exists independently. The two
///    emit modes never both fire on one region.
/// 3. **Single owner per member** (the admission). Each non-root member's owner is
///    the root if it has a direct `member → root` containment edge (store, funnel,
///    OR capture). Otherwise it is the **unique** interior container that holds
///    it. A member with **no** emittable interior container edge (funnel
///    containment from a `%del` or key read) refuses the whole subtree to Shared,
///    the always-legal baseline. So does a member with **two or more** non-root
///    containers and no root edge, whose owner is ambiguous.
///
///    A capture owner-edge is emittable for **every** capture kind.
///    `lower_lambda_expr` reloads a direct local from its binding slot, and an
///    upvalue or transitive capture from the constructing function's environment
///    (docs/impl/region/adopt.md). So emit capability holds the capture-adopt
///    contract (suppress ⊆ adopt), and no lowerability refusal exists.
///
///    The lifetime obligation (filter 1) keeps the cross-activation (upvalue)
///    family out, by construction. The lexically enclosing, forwarding lambda also
///    captures an upvalue member. So its tight last use lands at or past that
///    enclosing lambda's own node, after a nested root's in-body drop in
///    post-order. That refusal is required. The nested root's region is per call
///    of the enclosing closure. Claiming a member that survives across calls
///    would free it under the encloser's live env reference, and re-adopt an
///    already-Owned region on the next call. Only an owner that outlives every
///    capturer can own such a member: the activation or fiber owner node.
pub(in crate::hir::region::infer) fn compute_adopt_edges(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    arena: &BindingArena,
    order: &Order,
) -> AdoptEdges {
    let owned = compute_owned_subtrees(inputs, info);
    let aliases = AliasTables::new(info);
    // Capture containment edges `(lambda_id, captured, closure)`, re-derived because
    // a capture records no `cross_region_refs` edge. They are a second source of
    // interior owner-edges, emitted at the closure's construction site rather than a
    // store node. The shared inputs hold the one re-derivation for the whole pass.
    let all_captures = inputs.capture_edges();
    // Structural post-dominance over the scope tree, the lifetime obligation's
    // authority (docs/impl/region/adopt.md). Built once, queried per member below.
    let pd = PostDom::new(hir, order);
    // A region collapsed by a builder-idiom MERGE (as child or parent) belongs to
    // that mechanism and is never adopted. The parent side is the `merged_parent`
    // VALUE set, precomputed once so the per-member test below is O(1). Scanning
    // `.values()` per member costs O(members × merges) over the whole-stdlib letrec.
    let merged_parents: FxHashSet<Region> = info.merged_parent.values().copied().collect();
    let is_merged =
        |r: Region| -> bool { info.merged_parent.contains_key(&r) || merged_parents.contains(&r) };
    // Compiled capture-cell region → its binding. A `cell ⊇ content` containment
    // edge is one whose CONTAINER is a cell region. It is an emittable owner-edge
    // only for an IMMUTABLE letrec cell. A re-storable cell's content lifetime is
    // per-rebind, so adopting it into the cell's subtree would free a displaced
    // prior under the live cell (`tests/impl/region-capture-cell-loop-uaf.lisp`).
    let mut cell_binding_of: FxHashMap<Region, Binding> = FxHashMap::default();
    for cells in info.begin_cell_regions.values() {
        for &(b, cell_r) in cells {
            cell_binding_of.insert(cell_r, b);
        }
    }
    let adoptable_cell = |cell_r: Region| -> bool {
        cell_binding_of
            .get(&cell_r)
            .is_some_and(|&b| !arena.get(b).is_restorable_capture_cell())
    };

    // Interior-edge index. The accepted Owned subtrees PARTITION the regions: a region
    // reached from two roots fails external uniqueness for both (`subtree`). So each
    // interior region maps to exactly one root. Inverting `owned` into `root_of` once,
    // then bucketing every eligible edge under its interior root in one pass, costs
    // O(edges). A per-root scan of every edge would cost O(roots × edges), quadratic
    // on the stdlib's one giant letrec. An edge is interior to root R iff BOTH
    // endpoints are members of R's subtree: `root_of[src] == root_of[dst] == R`.
    let mut root_of: FxHashMap<Region, Region> = FxHashMap::default();
    for (&root, members) in &owned {
        for &m in members {
            let prev = root_of.insert(m, root);
            debug_assert!(
                prev.is_none(),
                "owned subtrees must be disjoint, but region r{} appears in two subtrees",
                m.0
            );
        }
    }
    let interior_root = |src: Region, dst: Region| -> Option<Region> {
        match (root_of.get(&src), root_of.get(&dst)) {
            (Some(&rs), Some(&rd)) if rs == rd => Some(rs),
            _ => None,
        }
    };
    // Per-root interior edge lists, built in source-list order so the emitted adopt order
    // follows the edge lists. Funnel and cell edges both come from `containment_edges` and
    // are bucketed in one pass; an edge can qualify for both.
    type EdgeList = Vec<(HirId, Region, Region)>;
    let mut store_by_root: FxHashMap<Region, EdgeList> = FxHashMap::default();
    for &(site, src, dst) in &info.cross_region_refs {
        if info.hard_edge_sites.contains(&site) {
            continue;
        }
        if let Some(r) = interior_root(src, dst) {
            store_by_root.entry(r).or_default().push((site, src, dst));
        }
    }
    let mut capture_by_root: FxHashMap<Region, EdgeList> = FxHashMap::default();
    for &(lambda, src, dst) in all_captures {
        if let Some(r) = interior_root(src, dst) {
            capture_by_root
                .entry(r)
                .or_default()
                .push((lambda, src, dst));
        }
    }
    let mut funnel_by_root: FxHashMap<Region, EdgeList> = FxHashMap::default();
    let mut cell_by_root: FxHashMap<Region, EdgeList> = FxHashMap::default();
    for &(site, src, dst) in &info.containment_edges {
        let Some(r) = interior_root(src, dst) else {
            continue;
        };
        if info
            .funnel_store_sites
            .get(&site)
            .is_some_and(|stored| stored.contains(&src))
        {
            funnel_by_root.entry(r).or_default().push((site, src, dst));
        }
        if adoptable_cell(dst) {
            cell_by_root.entry(r).or_default().push((site, src, dst));
        }
    }
    let no_edges: EdgeList = Vec::new();

    let mut out = AdoptEdges {
        store: HashMap::new(),
        capture: HashMap::new(),
        cell_content: Vec::new(),
    };
    for (&root, members) in &owned {
        // The root's single decref drops the whole subtree, so the root must have
        // release data, and (the lifetime obligation below) its free must
        // post-dominate every member's own region deref. The obligation is checked
        // AFTER owner assignment, because which last use bounds a member depends on
        // whether its own decref is suppressed (capture-adopted) or live
        // (store-adopted).
        let Some(root_dp_id) = info.region_data.get(&root).map(|d| d.lifetime_point) else {
            continue;
        };
        // No merge overlap: a region collapsed by a builder-idiom MERGE is owned by that
        // mechanism, never adopted (the two emit modes never both fire on one region).
        if members.iter().any(|&m| is_merged(m)) {
            continue;
        }
        // Interior containment edges with an emit site, from the per-root index above.
        // STORE edges carry their `cross_region_refs` store-site HirId, oriented
        // `(site, src=child, dst=parent)` for `target ⊇ source`. They are non-hard,
        // because a hard may-store does not build the subtree. CAPTURE edges carry the
        // Lambda's HirId. FUNNEL edges carry their funnel call-site HirId, bucketed only
        // at a retaining store. CELL edges (`cell ⊇ content`) are the immutable letrec
        // cells (`adoptable_cell`), adopted at the cell store keyed by its binding.
        let interior_store = store_by_root.get(&root).unwrap_or(&no_edges);
        let interior_capture = capture_by_root.get(&root).unwrap_or(&no_edges);
        let interior_funnel = funnel_by_root.get(&root).unwrap_or(&no_edges);
        let interior_cell = cell_by_root.get(&root).unwrap_or(&no_edges);
        // Assign each non-root member its single owner, its actual parent in the
        // containment graph (filter 3 in the function doc). The in-subtree containers
        // of `m` are the targets of interior edges sourced at `m`. Preferring the root
        // when it directly holds `m` means the root, not a sibling, adopts an interior
        // member↔member cycle among its direct children. The `lower_lambda_expr`
        // `debug_assert` is the backstop that every capture edge matches a real capture.
        let mut containers_of: FxHashMap<Region, FxHashSet<Region>> = FxHashMap::default();
        for &(_, src, dst) in interior_store
            .iter()
            .chain(interior_capture.iter())
            .chain(interior_funnel.iter())
            .chain(interior_cell.iter())
        {
            containers_of.entry(src).or_default().insert(dst);
        }
        let mut owner_of: FxHashMap<Region, Region> = FxHashMap::default();
        let mut refuse = false;
        for &m in members {
            if m == root {
                continue;
            }
            let owner = match containers_of.get(&m) {
                Some(cs) if cs.contains(&root) => root,
                Some(cs) if cs.len() == 1 => *cs.iter().next().unwrap(),
                _ => {
                    refuse = true;
                    break;
                }
            };
            // The `m → owner` edge must be EMITTABLE: an intrinsic store edge, a
            // retaining funnel edge, a capture edge (any kind), or a cell⊇content edge
            // (an immutable letrec cell). Funnel containment from a `%del` or key read
            // has none, and neither has a re-storable cell's content (`adoptable_cell`).
            // Such a member refuses the subtree to Shared rather than suppress its
            // release without an adopt, which would leak.
            let via_store = interior_store
                .iter()
                .chain(interior_funnel.iter())
                .chain(interior_cell.iter())
                .any(|&(_, s, d)| s == m && d == owner);
            let via_capture = interior_capture
                .iter()
                .any(|&(_, s, d)| s == m && d == owner);
            if !via_store && !via_capture {
                refuse = true;
                break;
            }
            owner_of.insert(m, owner);
        }
        if refuse {
            continue;
        }
        // Lifetime obligation (filter 1 in the function doc). A capture-adopted member
        // reads its TIGHT last use (`binding_last_use`): its release point sits one
        // structural step past its owning closure, but nothing derefs its region after
        // the subtree drop. A value also used after the closure keeps that later direct
        // use in the tight set and still refuses. A store-adopted member reads its
        // structural `lifetime_point`, because its own decref stays live
        // (`adopt_edges_refuses_captured_store_member_on_lifetime`).
        let suppressed = |m: Region| -> bool {
            owner_of.get(&m).is_some_and(|&owner| {
                interior_capture
                    .iter()
                    .any(|&(_, s, d)| s == m && d == owner)
                    && !interior_store
                        .iter()
                        .chain(interior_funnel.iter())
                        .any(|&(_, s, d)| s == m && d == owner)
            })
        };
        let obligation_lu = |m: Region| -> Option<HirId> {
            if suppressed(m) {
                info.binding_last_use
                    .get(&m)
                    .copied()
                    .or_else(|| info.region_data.get(&m).map(|d| d.lifetime_point))
            } else {
                info.region_data.get(&m).map(|d| d.lifetime_point)
            }
        };
        // The root's single decref must post-dominate every member's relevant last
        // use, decided STRUCTURALLY over the scope tree. `EmitMode::Adopt` adds the
        // loop clause: a store-adopted member keeps its own decref, so a loop
        // enclosing the root's free would re-deref the member after the drop. A
        // subtree with an un-post-dominated member stays Shared. A numeric
        // `ord(member) <= ord(root)` test would admit a free-before-use across
        // branches and loop back-edges, where order is not a post-dominance proxy.
        if !members.iter().all(|&m| {
            m == root
                || obligation_lu(m)
                    .is_some_and(|lu| pd.drop_post_dominates(root_dp_id, lu, EmitMode::Adopt))
        }) {
            continue;
        }
        // The same obligation over every alias of a member: a value read back out
        // of the subtree, an opaque call's result over it, and a release routed
        // through a member cell. The root's drop can tear each such region, and no
        // member's own point names it. So the drop must post-dominate its release
        // too. Otherwise the subtree refuses to Shared, where the pass-through
        // retain is live again (`AliasTables`).
        if !aliases
            .release_points(members, info)
            .iter()
            .all(|dp| dp.is_some_and(|lu| pd.drop_post_dominates(root_dp_id, lu, EmitMode::Adopt)))
        {
            continue;
        }
        // The admitted shape's numeric shadow: post-domination implies the member's
        // last use does not linearize after the root's free. A debug echo of the
        // structural verdict, never the deciding test.
        #[cfg(debug_assertions)]
        {
            let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
            let root_dp = ord(root_dp_id);
            for &m in members {
                if m == root {
                    continue;
                }
                if let Some(lu) = obligation_lu(m) {
                    debug_assert!(
                        ord(lu) <= root_dp,
                        "adopt admitted member r{} whose last use ({}) linearizes after \
                         the root r{}'s free ({root_dp})",
                        m.0,
                        ord(lu),
                        root.0,
                    );
                }
            }
        }
        // Emit one adopt per non-root member at its `member → owner` edge, routed to the
        // store or capture map by the edge kind. `adopted` (shared across the loops)
        // dedups a member adopted more than once, because the runtime sets the owner
        // edge once. An interior edge to a region that is NOT the source's chosen owner
        // (a cycle edge, or a redundant store/capture into a non-owner) carries no
        // adopt and needs none. A Fresh member has no populated static slot, so its
        // `IncrefRegion` is a runtime no-op, and the frozen-RC no-op absorbs any that
        // resolves. The cycle reclaims with the root's subtree drop
        // (docs/impl/region/rules.md).
        let mut adopted: FxHashSet<Region> = FxHashSet::default();
        for &(site, src, dst) in interior_store.iter().chain(interior_funnel.iter()) {
            if owner_of.get(&src) == Some(&dst) && adopted.insert(src) {
                out.store.entry(site).or_default().push((src, dst));
            }
        }
        for &(lambda, src, dst) in interior_capture {
            if owner_of.get(&src) == Some(&dst) && adopted.insert(src) {
                out.capture.entry(lambda).or_default().push((src, dst));
            }
        }
        // A `cell ⊇ content` adopt is emitted at the cell's own store site (not a
        // `cross_region_refs` node), keyed by the cell's binding: the lowerer emits
        // `AdoptCellRegion(cell, content)` there. The holding closure capture-adopts the
        // cell (dst) through `interior_capture` above, so the chain frees as one.
        for &(_site, src, dst) in interior_cell {
            if owner_of.get(&src) == Some(&dst) && adopted.insert(src) {
                if let Some(&binding) = cell_binding_of.get(&dst) {
                    out.cell_content.push(binding);
                }
            }
        }
    }
    out
}
