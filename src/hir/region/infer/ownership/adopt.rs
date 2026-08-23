use super::super::postdom::{EmitMode, PostDom};
use super::super::*;
use super::inputs::OwnershipInputs;
use super::subtree::compute_owned_subtrees;
use rustc_hash::{FxHashMap, FxHashSet};

/// The two adopt-edge maps the ownership forest's emit consumes, by emit site — the
/// output of `compute_adopt_edges` (docs/impl/region/ownership.md § "Adoption and subtree
/// drop"). Computed by the ownership pass in `analyze_regions_with`.
pub(in crate::hir::region::infer) struct AdoptEdges {
    /// `cross_region_refs` **store-site** HirId → `(member, owner)` adopts the lowerer
    /// emits in `emit_increfs_for` in place of the interior `IncrefRegion`
    /// (`RegionInfo::owned_adopt_edges`).
    pub store: HashMap<HirId, Vec<(Region, Region)>>,
    /// **Lambda** (closure-construction) HirId → `(captured, closure)` adopts the
    /// lowerer emits in `lower_lambda_expr` in place of the capture `IncrefRegion`
    /// (`RegionInfo::capture_adopt_edges`). Capture records no `cross_region_refs` store
    /// site, so its adopt is keyed by the closure rather than a store node. A member that
    /// is a **cell** region (a re-pointed `closure ⊇ cell` edge) is emitted as
    /// `AdoptCellRegion` (its own region, not the unwrapped content); a by-value member as
    /// `AdoptRegion` — `lower_lambda_expr` distinguishes by the captured binding.
    pub capture: HashMap<HirId, Vec<(Region, Region)>>,
    /// The bindings whose compiled capture cell adopts its stored **content** (a
    /// `cell ⊇ content` edge — `RegionInfo::cell_content_adopt_bindings`). At each such
    /// binding's cell-store site the lowerer emits `AdoptCellRegion(cell, content)`,
    /// linking the content's runtime region into the cell's own region (never unwrapped).
    /// Only immutable letrec cells reach here — a re-storable cell's content is refused
    /// (§3 loop hazard). The cell itself is capture-adopted into the holding closure via
    /// `capture` above, so the whole `closure ⊇ cell ⊇ content` chain frees as one subtree.
    pub cell_content: Vec<Binding>,
}

/// The interior containment edges to emit as `AdoptRegion`, split by emit site —
/// the ownership forest's emit input. Wraps [`compute_owned_subtrees`] with the
/// **lifetime obligation** the external-uniqueness walk does not itself enforce, then
/// assigns each non-root member a single owner and projects its owner-edge to the
/// appropriate emit site.
///
/// Each emitted `(site → [(member, owner), …])` says: at `site`, link `member`'s runtime
/// region into `owner`'s Owned subtree (`AdoptRegion(owner, member)`) instead of the
/// interior edge's `IncrefRegion`. Each non-root member is adopted by its **actual
/// parent** — the root when a direct `member → root` edge exists (a flat star, the common
/// case), otherwise the single interior container that holds it (multi-level nesting
/// `root ⊇ a ⊇ b`, where `a` adopts `b` and the root adopts `a`). The owner graph is a
/// forest — one owner per member — and the root's single decref subtree-drops the whole
/// component recursively (`free_runtime_region_pages` walks `owned_children`; pinned by
/// `regionstore::tests::subtree_drop_is_recursive`). An interior member↔member cycle edge
/// that is no member's chosen owner-edge carries no adopt and needs none — it reclaims
/// with the subtree drop.
///
/// **Three containment-edge kinds, three emit sites.** A `cross_region_refs` store edge
/// (`%pair`/`%array-push`/`%put`) carries its store-site HirId — its adopt rides
/// `AdoptEdges::store`, consumed by `emit_increfs_for`. A **capture** edge
/// (`closure ⊇ captured`) has no store site (the RC double-count fix records no
/// `cross_region_refs` edge), so its adopt rides `AdoptEdges::capture` keyed by the
/// closure's construction HirId, consumed at `MakeClosure` in `lower_lambda_expr`. A
/// **funnel-recovered** containment edge (`RegionInfo::containment_edges` — the storing
/// ops lower as opaque `Funnel` native calls recording no `cross_region_refs` edge)
/// carries its funnel **call-site** HirId and rides
/// `AdoptEdges::store` keyed there: the value-resolved adopt reloads both endpoints from
/// their binding slots, so it needs no store opcode — the same F-b face the activation
/// and transfer cuts carry (region/adopt.md § "The funnel adopt"). A funnel edge is
/// emittable only at a **retaining-store** site recording the
/// member as its stored value (`funnel_store_sites`; a `%del`/key read retains nothing
/// and stays unemittable). All three feed owner assignment, so a value captured by a
/// *local* Owned closure is adopted by the closure (the per-call closure↔capture knot
/// the forest exists to reclaim) and a funnel-built subtree reclaims exactly like a
/// constructor-built one.
///
/// Three filters beyond the external-uniqueness walk (checked in this order — the
/// lifetime obligation reads the owner assignment, so it runs *after* it):
/// 1. **Lifetime obligation** (the shared `postdom::drop_post_dominates`, `EmitMode::Adopt`),
///    **owner-aware**: the root's `decref_point` must **structurally post-dominate** every
///    member's relevant last use over the scope tree (not a numeric `ord` compare —
///    region/adopt.md § "The lifetime obligation the root carries") — else the root's single
///    decref subtree-drops a region still derefed afterward (a UAF), and across a branch arm or
///    loop back-edge order is not a post-dominance proxy. Which last-use bounds a member depends
///    on whether its own decref is SUPPRESSED. A **capture-adopted**
///    member's own decref is suppressed (it is freed only by the subtree drop), so its
///    over-extended `region_data` `decref_point` is harmless and the TIGHT last-use
///    (`binding_last_use`) admits the safe capture cut. A **store-adopted**
///    member keeps its own `DecrefValueRegion` at the structural `decref_point` (a Fresh
///    call-result is released value-based, the decref reading the region through
///    `result_region_of`), so it MUST be bounded by that structural point — the tight value
///    would unsoundly free a captured-AND-store-adopted member (the m↔c capture-back-edge
///    cycle) before its own trailing decref-value derefs the freed page. A subtree that fails
///    is dropped here (no region root owns it); the m↔c SCC itself is then claimed by the
///    activation cut (`compute_activation_adopts` — owner = activation), and only a shape
///    that cut also refuses stays Shared. The check is against the root
///    only: every member — at any depth — frees simultaneously at the root's drop.
///    The obligation covers one more deref the members' own points do not name: a value
///    read back OUT of the subtree by a native `get`/`first`/`rest`
///    (`RegionInfo::counted_read_aliases`). Its region is a fresh call-result placeholder
///    related to no member, and its Rule 5 pass-through retain — which is what makes the
///    read safe under RC — goes inert once adoption freezes the member. So the root's
///    drop must post-dominate the ALIAS's release too, else the subtree refuses to
///    Shared, where the retain is live again. An opaque CALL over a member
///    (`RegionInfo::opaque_result_aliases`) is the same deref reached one step further
///    out: its result may BE a member or the container, so it is closed into the same
///    alias set and bounded the same way.
/// 2. **No merge overlap**: a subtree touching any builder-idiom MERGE participant is
///    skipped — MERGE collapses those to one region; adopting them too would link a
///    region that no longer exists independently. The two emit modes never both fire
///    on one region.
/// 3. **Single owner per member** (the admission): each non-root member's owner is the
///    root if it has a direct `member → root` containment edge (store, funnel, OR
///    capture), else the **unique** interior container that holds it. A member with
///    **no** emittable interior container edge (e.g. funnel containment with no
///    retaining-store site — a `%del`/key read) or with **two or more** non-root
///    containers and no root edge (an ambiguous owner — which of them frees it?)
///    refuses the whole subtree to Shared (the always-legal baseline).
///    A capture owner-edge is emittable for **every** capture kind — `lower_lambda_expr`
///    reloads a direct local from its binding slot and an upvalue/transitive capture from
///    the constructing function's environment (region/adopt.md § "The capture adopt") —
///    so the capture-adopt contract (suppress ⊆ adopt) is held by emit capability, and no
///    lowerability refusal exists. The **lifetime obligation** (filter 1) is what keeps
///    the cross-activation (upvalue) family out, by construction: an upvalue member is
///    also captured by the lexically enclosing (forwarding) lambda, so its tight last-use
///    lands at or past that enclosing lambda's own node — after a nested root's in-body
///    drop in post-order. That refusal is genuine, not conservatism: the nested root's
///    region is per-call of the enclosing closure, so claiming a member that survives
///    across calls would free it under the encloser's live env reference and re-adopt an
///    already-Owned region on the next call. Such a member is ownable only by an owner
///    that outlives every capturer (the activation/fiber owner node).
pub(in crate::hir::region::infer) fn compute_adopt_edges(
    inputs: &OwnershipInputs,
    hir: &Hir,
    info: &RegionInfo,
    arena: &BindingArena,
    order: &HashMap<HirId, u32>,
) -> AdoptEdges {
    let owned = compute_owned_subtrees(inputs, info);
    // The read/funnel alias tables, indexed by source/container, are built once
    // for the whole ownership pass. The per-root fixpoint below walks only the
    // edges out of the regions its reachable set actually touches.
    let mut aliases_by_source: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
    for &(_, alias, source) in info
        .counted_read_aliases
        .iter()
        .chain(info.opaque_result_aliases.iter())
    {
        aliases_by_source.entry(source).or_default().push(alias);
    }
    let mut funnel_by_container: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
    for &(_, result, container) in &info.funnel_result_containers {
        funnel_by_container
            .entry(container)
            .or_default()
            .push(result);
    }
    // Capture containment edges `(lambda_id, captured, closure)` — re-derived (capture
    // records no `cross_region_refs` edge), so they are a second source of interior
    // owner-edges, emitted at the closure's construction site rather than a store node.
    // Read from the shared inputs (the ONE re-derivation for the whole ownership pass).
    let all_captures = inputs.capture_edges();
    // Structural post-dominance over the scope tree — the lifetime obligation's
    // authority (region/adopt.md § "The lifetime obligation the root carries").
    // Built once, queried per member below.
    let pd = PostDom::new(hir, order);
    // A region collapsed by a builder-idiom MERGE (as child or parent) is owned by
    // that mechanism, never adopted. The parent side is the `merged_parent` VALUE set,
    // precomputed once so the per-member test below is O(1) — scanning `.values()` per
    // member made the merge-overlap filter O(members × merges) over the whole-stdlib letrec.
    let merged_parents: FxHashSet<Region> = info.merged_parent.values().copied().collect();
    let is_merged =
        |r: Region| -> bool { info.merged_parent.contains_key(&r) || merged_parents.contains(&r) };
    // Compiled capture-cell region → its binding, and the set of all such regions. A
    // `cell ⊇ content` containment edge is one whose CONTAINER is a cell region; it is
    // an emittable owner-edge only for an IMMUTABLE letrec cell — a re-storable cell's
    // content lifetime is per-rebind, so adopting it into the cell's subtree would free a
    // displaced prior under the live cell (§3 loop hazard; `region-capture-cell-loop-uaf`).
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

    // Interior-edge index. The accepted Owned subtrees PARTITION the regions (subtree.rs:
    // a region reached from two roots fails external uniqueness for both, so no region is a
    // member of two subtrees), so each interior region maps to exactly one root. Inverting
    // `owned` into `root_of` once, then bucketing every eligible edge under its (unique)
    // interior root in a single pass, replaces the per-root re-scan of EVERY
    // cross_region_refs / capture / containment edge — which was O(roots × edges), quadratic
    // on the stdlib's one giant letrec — with O(edges). An edge is interior to root R iff
    // BOTH endpoints are members of R's subtree, i.e. `root_of[src] == root_of[dst] == R`.
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
    // is identical to the pre-index per-root filter. Funnel and cell edges both come from
    // `containment_edges` and are bucketed in one pass (an edge can qualify for both, exactly
    // as the two independent per-root filters admitted).
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
        // The root's single decref drops the whole subtree, so the root must have a
        // `decref_point`, and (the lifetime obligation below) its free must
        // post-dominate every member's own region deref. The obligation is checked
        // AFTER owner assignment, because which last-use bounds a member depends on
        // whether its own decref is suppressed (capture-adopted) or live
        // (store-adopted) — see the obligation block.
        let Some(root_dp_id) = info.region_data.get(&root).map(|d| d.lifetime_point) else {
            continue;
        };
        // No merge overlap: a region collapsed by a builder-idiom MERGE is owned by that
        // mechanism, never adopted (the two emit modes never both fire on one region).
        if members.iter().any(|&m| is_merged(m)) {
            continue;
        }
        // Interior containment edges with an emit site, looked up from the per-root index
        // built above. STORE edges carry their `cross_region_refs` store-site HirId
        // (orientation `(site, src=child, dst=parent)` — `target ⊇ source`), non-hard (a hard
        // may-store does not build the subtree). CAPTURE edges carry the Lambda's HirId as
        // their site. FUNNEL edges carry their funnel call-site HirId — bucketed only where
        // the site is a retaining store recording the member as its stored value (a `%del`/key
        // containment edge retains nothing). CELL edges (`cell ⊇ content`) are the immutable
        // letrec cells (`adoptable_cell`), value-resolved at the cell store as
        // `AdoptCellRegion(cell, content)` keyed by the cell's binding.
        let interior_store = store_by_root.get(&root).unwrap_or(&no_edges);
        let interior_capture = capture_by_root.get(&root).unwrap_or(&no_edges);
        let interior_funnel = funnel_by_root.get(&root).unwrap_or(&no_edges);
        let interior_cell = cell_by_root.get(&root).unwrap_or(&no_edges);
        // Assign each non-root member its single owner — its **actual parent** in the
        // containment graph (store + capture edges both count). The in-subtree containers
        // of `m` are the targets of interior edges sourced at `m`. Prefer the root when it
        // directly holds `m` (the flat-star case — an interior member↔member cycle among
        // the root's direct children is then adopted by the root, not by a sibling), else
        // take the unique non-root container (multi-level nesting `root ⊇ a ⊇ b`: `b`'s
        // only container is `a`, so `a` adopts `b`). A member with NO emittable container
        // edge (funnel-only, no store site) or with two-or-more non-root containers and no
        // root edge (an ambiguous owner — which frees it?) refuses the whole subtree to
        // Shared, the always-legal baseline.
        //
        // **The capture-adopt contract** (suppress ⊆ adopt) is held by emit capability: a
        // chosen CAPTURE owner-edge is emittable for EVERY capture kind —
        // `lower_lambda_expr` reloads a direct local from its binding slot and an
        // upvalue/transitive capture from the constructing function's environment
        // (region/adopt.md § "The capture adopt") — so no lowerability refusal is needed
        // here, and the `lower_lambda_expr` `debug_assert` is the backstop that every edge
        // matches a real capture. What keeps the cross-activation (upvalue) family out is
        // the lifetime obligation below, by construction — see the single-owner filter's
        // doc on `compute_adopt_edges`.
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
            // (an immutable letrec cell). A member with none (funnel containment with no
            // retaining site — a `%del`/key read; or a re-storable cell's content, whose
            // edge `adoptable_cell` excludes) refuses the subtree to Shared (the
            // always-legal baseline) rather than suppress-without-adopt (a leak).
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
        // Lifetime obligation (owner-aware): the root's single decref drops the whole
        // subtree, so it must not precede any member's own region deref. WHICH last-use
        // bounds a member depends on whether its own decref is SUPPRESSED:
        //
        // - A **capture-adopted** member (its chosen owner-edge is a capture, so
        //   `analyze_regions_with` suppresses its own decref) is freed ONLY by the subtree
        //   drop — nothing derefs its region afterward. Its `region_data` `decref_point` is
        //   over-extended one structural step past its owning closure, so
        //   the TIGHT last-use (`binding_last_use`) is read instead, admitting the safe
        //   capture cut; a value also used after the closure keeps that later direct use in
        //   the tight set and is still refused.
        // - A **store-adopted** member keeps its OWN `DecrefValueRegion`/`DecrefRegion` at
        //   its structural `decref_point` (a Fresh call-result is released value-based, the
        //   decref reading the region through `result_region_of`). If the root's drop frees
        //   that region first, the member's own later decref derefs a freed page — a UAF. So
        //   a store-adopted member MUST be bounded by its STRUCTURAL `decref_point`, never the
        //   tight last-use. This bites a captured-AND-store-adopted member — a container
        //   captured by a closure but owned by an outer container, i.e. the m↔c
        //   capture-back-edge cycle (`adopt_edges_refuses_captured_store_member_on_lifetime`):
        //   the tight value would unsoundly admit it and free it before its over-extended
        //   decref-value. No region root owns such a member; the m↔c SCC is claimed by the
        //   activation cut instead (`compute_activation_adopts` — owner = activation).
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
        // use — decided STRUCTURALLY over the scope tree (`EmitMode::Adopt`: a
        // store-adopted member keeps its own decref, so a loop enclosing the root's
        // free re-derefs the member after the drop — the cross-iteration UAF the
        // loop clause refuses). A subtree with an un-post-dominated member stays
        // Shared (the always-legal baseline). The category error this replaces: a
        // numeric `ord(member) <= ord(root)` admits a free-before-use across branches
        // and loop back-edges, where order is not a post-dominance proxy.
        if !members.iter().all(|&m| {
            m == root
                || obligation_lu(m)
                    .is_some_and(|lu| pd.drop_post_dominates(root_dp_id, lu, EmitMode::Adopt))
        }) {
            continue;
        }
        // The same obligation over the values read back OUT of this subtree. A native
        // container read (`get`/`first`/`rest` — `counted_read_aliases`) hands the reader
        // a value that still lives inside the container, under a **counted** pass-through
        // retain (Rule 5) the RC baseline honours — but adoption FREEZES the member's RC,
        // so that retain buys nothing and the root's drop reclaims the element under the
        // reader (whose own value-resolved release then faults on the freed page). The
        // alias's region is a fresh call-result placeholder no containment edge relates to
        // any member, so nothing above sees it: check it here. A read whose alias the
        // root's drop does not post-dominate refuses the subtree to Shared, where the
        // retain is live again (region/adopt.md § "The lifetime obligation the root
        // carries"; `region_container_read_borrow_uaf`).
        //
        // Aliasing is TRANSITIVE: reading out of an alias reaches deeper into the same
        // subtree (`(get (get c 0) 0)` names a value inside a member of a member), and
        // the inner alias's own bound says nothing about the outer one's. Close the read
        // edges over the member set to a fixpoint — bounded by the alias count, since
        // each round adds at least one region — so every value reachable from the subtree
        // through any chain of reads is bound by the root's drop.
        //
        // An opaque CALL closes in the same fixpoint (`opaque_result_aliases`). Its
        // result is under no declaration that it lives in the call's own region, so it
        // may BE an argument (`concat` returns a mutable first argument in place) or a
        // value read out of one (`last`) — one edge covering both, because both make the
        // result a region inside the argument's subtree. Reaching it therefore bounds its
        // own release (it may be a member) AND lets the read edges reach on through it
        // (it may be the container), which no single-relation closure would give.
        //
        // A `Funnel`'s result (`funnel_result_containers`) reaches just as far and needs
        // no bound: the declaration pins it to arg0 in place or a fresh copy — the
        // container, never an interior element — so it carries arg0's own counted
        // reference where it is the root, and no-ops on the frozen region where arg0 is
        // itself a member (the emit-order paragraph in region/adopt.md). Bounding it
        // would refuse every mutable-store subtree over its own trailing discarded store
        // result.
        //
        // Reachability and boundedness are tracked SEPARATELY: a region can be reachable
        // without needing a bound (a funnel result), and one relation must not consume
        // the other's obligation — a read alias that a funnel edge happened to make
        // reachable first still owes its own bound.
        let mut reachable: FxHashSet<Region> = members.clone();
        let mut bounded: FxHashSet<Region> = FxHashSet::default();
        // Each alias's own release point — `None` where none is recorded, which bounds
        // nothing and so refuses, exactly as an unbounded member does.
        let mut alias_dps: Vec<Option<HirId>> = Vec::new();
        let mut worklist: Vec<Region> = reachable.iter().copied().collect();
        while let Some(src) = worklist.pop() {
            if let Some(aliases) = aliases_by_source.get(&src) {
                for &alias in aliases {
                    // Bounding is per-edge, not per-new-reach: a read alias that a
                    // funnel edge happened to make reachable first still owes its own
                    // bound, so `bounded`/`alias_dps` must see every alias whose
                    // source is reachable, regardless of which edge grew `reachable`.
                    if bounded.insert(alias) {
                        alias_dps.push(info.region_data.get(&alias).map(|d| d.lifetime_point));
                    }
                    if reachable.insert(alias) {
                        worklist.push(alias);
                    }
                }
            }
            if let Some(results) = funnel_by_container.get(&src) {
                for &result in results {
                    if reachable.insert(result) {
                        worklist.push(result);
                    }
                }
            }
        }
        if !alias_dps
            .iter()
            .all(|dp| dp.is_some_and(|lu| pd.drop_post_dominates(root_dp_id, lu, EmitMode::Adopt)))
        {
            continue;
        }
        // The admitted shape's numeric shadow: post-domination implies the member's
        // last use does not linearize after the root's free. A debug echo of the
        // structural verdict, never the deciding test — a drift back to numeric-only
        // admission detonates here.
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
        // store or capture map by the edge kind. `adopted` (shared across both loops)
        // dedups a member adopted more than once — the runtime owner edge is set once. An
        // interior edge to a region that is NOT the source's chosen owner (a cycle edge,
        // or a redundant store/capture into a non-owner) carries no adopt and needs none:
        // a Fresh member has no populated static slot so its `IncrefRegion` is a runtime
        // no-op, and the frozen-RC no-op absorbs any that resolves — the cycle reclaims
        // with the root's subtree drop (region/rules.md Rule 8).
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
        // `AdoptCellRegion(cell, content)` there. The cell (dst) is itself capture-adopted
        // into the holding closure via `interior_capture` above, so the chain frees as one.
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
