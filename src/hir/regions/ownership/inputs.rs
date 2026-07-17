use super::super::*;
use super::capture::capture_containment_edges;
use super::seeds::compute_shared_seeds;
use rustc_hash::{FxHashMap, FxHashSet};

/// The shared containment-graph + ownership-candidate inputs read by BOTH the
/// externally-unique subtree walk (`compute_owned_subtrees`) and the co-owned-cycle
/// walk (`compute_owned_region_groups`): the eligible-edge child map, the `contained`
/// set, the Shared-seed set, the re-derived capture edges, the holder index, and the
/// real-allocation candidate set. Factored so the two walks read the *same* graph by
/// construction — a divergence between them would be an ownership-soundness bug.
pub(in crate::hir::regions) struct OwnershipInputs {
    /// `parent → children` over ELIGIBLE containment edges (non-hard `cross_region_refs`
    /// stores + capture + funnel-recovered containment): a parent contains each child
    /// (`target ⊇ source`). A `hard_edge_site` may-store does not build the subtree (it
    /// still counts against external uniqueness — see [`Self::outside_ref_in`]).
    children_of: FxHashMap<Region, Vec<Region>>,
    /// Every region that is some parent's child — i.e. not a top container.
    pub(super) contained: FxHashSet<Region>,
    /// The Shared-seed set (`compute_shared_seeds`): regions that cross a frontier.
    shared: FxHashSet<Region>,
    /// Region → distinct user holders, for the sole-held check.
    region_holders: super::super::holders::RegionHolders,
    /// Real allocations (`alloc_region` sites + pre-allocated capture cells) — the
    /// candidate region set both walks iterate.
    pub(super) alloc_regions: FxHashSet<Region>,
    /// `source → targets` over ALL edges the external-uniqueness scan reads (every
    /// `cross_region_refs` store — hard edges INCLUDED — plus the capture and funnel
    /// containment edges). Indexing by source once lets [`Self::outside_ref_in`] examine
    /// only the edges leaving a subtree's own members, instead of re-scanning every edge in
    /// the compilation unit per candidate subtree (which was O(subtrees × edges), quadratic
    /// on the whole-stdlib letrec).
    out_edges_by_src: FxHashMap<Region, Vec<Region>>,
    /// The re-derived capture containment edges `(lambda_id, captured, closure)`
    /// ([`capture_containment_edges`]). Held here so every ownership pass reads the ONE
    /// re-derivation — the HIR walk is not cheap on the whole-stdlib letrec, and it was
    /// previously run once per `ownership_inputs` build AND again directly in each of the
    /// adopt/transfer/activation passes. The passes need the raw edge list (not just the
    /// containment graph it feeds), so it is exposed via [`Self::capture_edges`].
    capture_edges: Vec<(HirId, Region, Region)>,
    /// Strongly-connected components of the eligible containment graph (`children_of`,
    /// `parent → child`), computed ONCE by one Tarjan pass. `comp_id` maps each node to its
    /// component's index in `comp_members`. Both the co-owned-group walk
    /// (`compute_owned_region_groups`) and the activation-adopt walk
    /// (`compute_activation_adopts`) need the SCC of each candidate region; computing it as
    /// `{m ∈ reach(r) : r ∈ reach(m)}` was a reachability closure PER region — O(regions ×
    /// subtree) on the whole-stdlib letrec. A single Tarjan pass over the same graph yields
    /// every region's component at once (O(nodes + edges)); [`Self::scc_of`] is the lookup.
    comp_id: FxHashMap<Region, usize>,
    /// The members of each component, indexed by `comp_id`'s value. Every node in
    /// `children_of` and every `alloc_regions` candidate has an entry (an isolated region is
    /// its own singleton component), so `scc_of` on any candidate root resolves.
    comp_members: Vec<FxHashSet<Region>>,
}

/// Build the [`OwnershipInputs`] for a compilation unit (the containment graph and the
/// candidate set), shared by ALL of the ownership passes — built once per compile in
/// `apply_ownership` and threaded by reference into each. Mirrors `build_info`'s
/// `cross_region_refs` filtering: only live source regions yield capture edges (done in
/// [`capture_containment_edges`]).
pub(in crate::hir::regions) fn ownership_inputs(
    hir: &Hir,
    info: &RegionInfo,
    escape: &crate::hir::EscapeInfo,
    arena: &BindingArena,
) -> OwnershipInputs {
    let capture_edges = capture_containment_edges(hir, info, arena);
    let shared = compute_shared_seeds(info, escape);
    // Region → distinct user holders (any non-synthetic user binding is a holder), the
    // shared sole-held index (`regions::holders`).
    let region_holders = super::super::holders::RegionHolders::from_source_regions(
        &info.binding_source_regions,
        arena,
        |_| true,
    );
    // The eligible containment edges (`target ⊇ source`): non-hard stores, capture, and
    // funnel-recovered containment. `children_of[parent]` lists a parent's children;
    // `contained` is every child (a non-top-container).
    let mut children_of: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
    let mut contained: FxHashSet<Region> = FxHashSet::default();
    for &(site, src, dst) in &info.cross_region_refs {
        if info.hard_edge_sites.contains(&site) {
            continue;
        }
        children_of.entry(dst).or_default().push(src);
        contained.insert(src);
    }
    for &(_lambda, src, dst) in &capture_edges {
        children_of.entry(dst).or_default().push(src);
        contained.insert(src);
    }
    for &(_site, src, dst) in &info.containment_edges {
        children_of.entry(dst).or_default().push(src);
        contained.insert(src);
    }
    let alloc_regions: FxHashSet<Region> = info
        .alloc_region
        .values()
        .copied()
        .chain(
            info.begin_cell_regions
                .values()
                .flat_map(|v| v.iter().map(|(_, r)| *r)),
        )
        .collect();
    // Source-indexed edges for the external-uniqueness scan. Unlike `children_of` (subtree
    // BUILD, hard edges excluded), this includes HARD `cross_region_refs` stores — a
    // may-store from outside still breaks external uniqueness (see `outside_ref_in`).
    let mut out_edges_by_src: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
    for &(_site, src, dst) in &info.cross_region_refs {
        out_edges_by_src.entry(src).or_default().push(dst);
    }
    for &(_lambda, src, dst) in &capture_edges {
        out_edges_by_src.entry(src).or_default().push(dst);
    }
    for &(_site, src, dst) in &info.containment_edges {
        out_edges_by_src.entry(src).or_default().push(dst);
    }
    let (comp_id, comp_members) = compute_sccs(&children_of, &alloc_regions);
    OwnershipInputs {
        children_of,
        contained,
        shared,
        region_holders,
        alloc_regions,
        out_edges_by_src,
        capture_edges,
        comp_id,
        comp_members,
    }
}

/// One iterative Tarjan pass over the eligible containment graph (`children_of`,
/// `parent → child`), returning each node's component index and the component members.
///
/// The seed set is every graph node PLUS every `alloc_regions` candidate, so an isolated
/// candidate (no containment edge) still gets its own singleton component and `scc_of`
/// resolves for it. Iterative (an explicit frame stack, not recursion): the whole-stdlib
/// letrec's containment chains can be deep enough to blow a recursive DFS's call stack.
///
/// The component of `r` equals `{m : r reaches m and m reaches r}` — exactly the set the
/// group/activation walks previously computed as `{m ∈ reach(r) : r ∈ reach(m)}`, so this
/// is a behaviour-preserving replacement of that per-region reachability closure.
// `map_entry` does not apply: the child step branches ON `index_of`'s membership — an
// absent child is a tree edge (insert + push onto both the Tarjan and the DFS-frame
// stacks), a present child is a back edge (a distinct lowlink update, only when still
// on-stack). The entry API expresses neither the else-if nor the vacant-arm side effects.
#[allow(clippy::map_entry)]
fn compute_sccs(
    children_of: &FxHashMap<Region, Vec<Region>>,
    alloc_regions: &FxHashSet<Region>,
) -> (FxHashMap<Region, usize>, Vec<FxHashSet<Region>>) {
    // Seed nodes: graph endpoints ∪ candidate roots (dedup, insertion order irrelevant —
    // SCCs are order-independent).
    let mut nodes: Vec<Region> = Vec::new();
    let mut seen: FxHashSet<Region> = FxHashSet::default();
    for (&p, kids) in children_of {
        if seen.insert(p) {
            nodes.push(p);
        }
        for &c in kids {
            if seen.insert(c) {
                nodes.push(c);
            }
        }
    }
    for &r in alloc_regions {
        if seen.insert(r) {
            nodes.push(r);
        }
    }

    let mut index_of: FxHashMap<Region, u32> = FxHashMap::default();
    let mut lowlink: FxHashMap<Region, u32> = FxHashMap::default();
    let mut on_stack: FxHashSet<Region> = FxHashSet::default();
    let mut tarjan_stack: Vec<Region> = Vec::new();
    let mut comp_id: FxHashMap<Region, usize> = FxHashMap::default();
    let mut comp_members: Vec<FxHashSet<Region>> = Vec::new();
    let mut next_index: u32 = 0;
    let empty: Vec<Region> = Vec::new();

    for &root in &nodes {
        if index_of.contains_key(&root) {
            continue;
        }
        // Explicit DFS frames `(node, next-child-cursor)`.
        index_of.insert(root, next_index);
        lowlink.insert(root, next_index);
        next_index += 1;
        tarjan_stack.push(root);
        on_stack.insert(root);
        let mut work: Vec<(Region, usize)> = vec![(root, 0)];
        while let Some(&(v, ci)) = work.last() {
            let kids = children_of.get(&v).unwrap_or(&empty);
            if ci < kids.len() {
                work.last_mut().unwrap().1 += 1;
                let w = kids[ci];
                if !index_of.contains_key(&w) {
                    // Tree edge: descend into an unvisited child.
                    index_of.insert(w, next_index);
                    lowlink.insert(w, next_index);
                    next_index += 1;
                    tarjan_stack.push(w);
                    on_stack.insert(w);
                    work.push((w, 0));
                } else if on_stack.contains(&w) {
                    // Back/cross edge to a node still on the stack: pull v's lowlink down.
                    let lw = index_of[&w];
                    if lw < lowlink[&v] {
                        lowlink.insert(v, lw);
                    }
                }
            } else {
                // v's children are exhausted. If it roots an SCC, pop the component.
                if lowlink[&v] == index_of[&v] {
                    let mut comp: FxHashSet<Region> = FxHashSet::default();
                    loop {
                        let w = tarjan_stack.pop().unwrap();
                        on_stack.remove(&w);
                        comp.insert(w);
                        comp_id.insert(w, comp_members.len());
                        if w == v {
                            break;
                        }
                    }
                    comp_members.push(comp);
                }
                work.pop();
                // Returning from v to its parent: propagate v's lowlink upward.
                if let Some(&(parent, _)) = work.last() {
                    let lv = lowlink[&v];
                    if lv < lowlink[&parent] {
                        lowlink.insert(parent, lv);
                    }
                }
            }
        }
    }
    (comp_id, comp_members)
}

impl OwnershipInputs {
    /// The re-derived capture containment edges (`closure ⊇ captured`) — the passes read
    /// this ONE derivation rather than re-walking the HIR (see the field doc).
    pub(super) fn capture_edges(&self) -> &[(HirId, Region, Region)] {
        &self.capture_edges
    }

    /// The Shared-seed set (frontier crossings). Exposed so the transfer cut reads the
    /// same seeds this input already computed, not a second `compute_shared_seeds`.
    pub(super) fn shared(&self) -> &FxHashSet<Region> {
        &self.shared
    }

    /// A region is never statically ownable when it crosses a frontier (a Shared seed) or
    /// has a runtime-determined identity/lifetime: a non-`Fresh` `call_result` placeholder
    /// (a possible borrow / opaque result — but a `Fresh` call-result is a genuinely
    /// caller-owned allocation and IS ownable), a `cell_release` placeholder, a reassigned
    /// `mutated_binding_value` region, a `suppressed_decref` region, or a region holding a
    /// **fiber** (`fiber_result_regions` — a fiber acquires aliases through runtime
    /// scheduler machinery no structural obligation can bound, so the forest is rooted at
    /// fibers and never claims one as a member; docs/impl/region/adopt.md § "The fiber
    /// member — refused at the class level").
    pub(super) fn not_ownable(&self, info: &RegionInfo, r: Region) -> bool {
        self.shared.contains(&r)
            || (info.call_result_regions.contains(&r) && !info.fresh_result_regions.contains(&r))
            || info.cell_release_regions.contains(&r)
            || info.mutated_binding_value_regions.contains(&r)
            || info.suppressed_decref_regions.contains(&r)
            || info.fiber_result_regions.contains(&r)
    }

    /// Sole-held: at most one distinct user binding holds `r` (an ANF-temp-only region
    /// has no user holder and is sole-held by construction).
    pub(super) fn sole_held(&self, r: Region) -> bool {
        self.region_holders
            .holders_of(r)
            .is_none_or(|hs| hs.len() <= 1)
    }

    /// The single user binding holding `r`, when exactly one does (`None` for a
    /// holderless or aliased region). The activation-adopt cut requires each SCC
    /// member to have its OWN holder binding — the slot its value-resolved
    /// `AdoptIntoActivation` loads — pairwise distinct across the members.
    pub(super) fn sole_holder(&self, r: Region) -> Option<Binding> {
        self.region_holders.holders_of(r).and_then(|hs| {
            if hs.len() == 1 {
                hs.iter().next().copied()
            } else {
                None
            }
        })
    }

    /// The strongly-connected component of `r` in the containment graph — every region
    /// mutually reachable with `r` (`r` reaches it AND it reaches `r`), `r` included. This
    /// is exactly `{m ∈ reach(r) : r ∈ reach(m)}`, but read from the one shared Tarjan pass
    /// instead of a per-region reachability closure. `r` must be a graph node or an
    /// `alloc_regions` candidate (every such region has a component); a genuine cycle is a
    /// component of size ≥ 2.
    pub(super) fn scc_of(&self, r: Region) -> &FxHashSet<Region> {
        let id = self.comp_id[&r];
        &self.comp_members[id]
    }

    /// `r` plus every region transitively contained in it, following the eligible edges
    /// from a parent in the set to its children. A set closure, so an interior reference
    /// cycle terminates.
    pub(super) fn reach(&self, r: Region) -> FxHashSet<Region> {
        let mut set: FxHashSet<Region> = FxHashSet::default();
        set.insert(r);
        let mut work = vec![r];
        while let Some(parent) = work.pop() {
            if let Some(kids) = self.children_of.get(&parent) {
                for &c in kids {
                    if set.insert(c) {
                        work.push(c);
                    }
                }
            }
        }
        set
    }

    /// External-uniqueness failure: some edge — over ALL `cross_region_refs` (hard edges
    /// included), the capture edges, and the funnel containment — has its source INSIDE
    /// `set` and its target OUTSIDE it (an outside container holding an interior region,
    /// which freeing `set` as a unit would dangle). Reads the source-indexed `out_edges_by_src`
    /// so only edges LEAVING `set`'s own members are examined (not every edge in the unit).
    pub(super) fn outside_ref_in(&self, set: &FxHashSet<Region>) -> bool {
        set.iter().any(|m| {
            self.out_edges_by_src
                .get(m)
                .is_some_and(|dsts| dsts.iter().any(|d| !set.contains(d)))
        })
    }
}

#[cfg(test)]
mod tests {
    //! [`compute_sccs`] replaces the per-region reachability closure the co-owned-group and
    //! activation walks used to run — `{m ∈ reach(r) : r ∈ reach(m)}`. These tests pin the
    //! Tarjan output against exactly that brute-force definition over a battery of small
    //! graphs, so any drift in the iterative Tarjan (the classic lowlink/back-edge bugs)
    //! detonates here rather than as a mis-owned region under the full stdlib.
    use super::*;

    /// `r` plus every region reachable from it following `children_of` (the same walk
    /// [`OwnershipInputs::reach`] does), computed independently of Tarjan.
    fn reach(children_of: &FxHashMap<Region, Vec<Region>>, r: Region) -> FxHashSet<Region> {
        let mut set = FxHashSet::default();
        set.insert(r);
        let mut work = vec![r];
        while let Some(p) = work.pop() {
            if let Some(kids) = children_of.get(&p) {
                for &c in kids {
                    if set.insert(c) {
                        work.push(c);
                    }
                }
            }
        }
        set
    }

    /// The SCC of `r` by the definition the walks used before Tarjan: every node mutually
    /// reachable with `r`.
    fn brute_scc(children_of: &FxHashMap<Region, Vec<Region>>, r: Region) -> FxHashSet<Region> {
        let from_r = reach(children_of, r);
        from_r
            .iter()
            .copied()
            .filter(|&m| reach(children_of, m).contains(&r))
            .collect()
    }

    fn graph(edges: &[(u32, u32)]) -> FxHashMap<Region, Vec<Region>> {
        let mut g: FxHashMap<Region, Vec<Region>> = FxHashMap::default();
        for &(p, c) in edges {
            g.entry(Region(p)).or_default().push(Region(c));
        }
        g
    }

    /// For every node in `nodes`, Tarjan's component must equal the brute-force SCC.
    fn assert_matches_brute(edges: &[(u32, u32)], nodes: &[u32]) {
        let children_of = graph(edges);
        let alloc: FxHashSet<Region> = nodes.iter().map(|&n| Region(n)).collect();
        let (comp_id, comp_members) = compute_sccs(&children_of, &alloc);
        for &n in nodes {
            let r = Region(n);
            let id = *comp_id
                .get(&r)
                .unwrap_or_else(|| panic!("region r{n} has no component"));
            let got = &comp_members[id];
            let want = brute_scc(&children_of, r);
            assert_eq!(
                *got, want,
                "SCC of r{n} disagrees with brute-force mutual reachability",
            );
        }
    }

    #[test]
    fn dag_all_singletons() {
        // 1→2, 1→3, 2→3: no cycles, every component a singleton.
        assert_matches_brute(&[(1, 2), (1, 3), (2, 3)], &[1, 2, 3]);
    }

    #[test]
    fn two_cycle() {
        // 1↔2: one component {1,2}.
        assert_matches_brute(&[(1, 2), (2, 1)], &[1, 2]);
    }

    #[test]
    fn three_cycle() {
        // 1→2→3→1: one component {1,2,3}.
        assert_matches_brute(&[(1, 2), (2, 3), (3, 1)], &[1, 2, 3]);
    }

    #[test]
    fn cycle_with_downstream_tail() {
        // 1↔2 and 2→3 (3 downstream, not in the cycle): {1,2} and {3}.
        assert_matches_brute(&[(1, 2), (2, 1), (2, 3)], &[1, 2, 3]);
    }

    #[test]
    fn two_disjoint_cycles() {
        // 1↔2 and 3↔4: two independent components.
        assert_matches_brute(&[(1, 2), (2, 1), (3, 4), (4, 3)], &[1, 2, 3, 4]);
    }

    #[test]
    fn nested_cycles_share_a_node() {
        // 1↔2, 2↔3 (2 shared): all three are mutually reachable → {1,2,3}.
        assert_matches_brute(&[(1, 2), (2, 1), (2, 3), (3, 2)], &[1, 2, 3]);
    }

    #[test]
    fn self_loop_is_singleton_component() {
        // A region storing into itself is still a size-1 component (the walks want ≥2 for a
        // genuine cycle, so a self-loop must not read as a two-member SCC).
        assert_matches_brute(&[(1, 1)], &[1]);
    }

    #[test]
    fn isolated_alloc_candidate_gets_a_component() {
        // An alloc-region candidate absent from the graph still resolves to its own
        // singleton component (so `scc_of` never panics on a lone candidate).
        let children_of = graph(&[(1, 2), (2, 1)]);
        let alloc: FxHashSet<Region> = [Region(1), Region(2), Region(9)].into_iter().collect();
        let (comp_id, comp_members) = compute_sccs(&children_of, &alloc);
        let id = comp_id[&Region(9)];
        assert_eq!(comp_members[id], [Region(9)].into_iter().collect());
    }

    #[test]
    fn deep_chain_does_not_overflow() {
        // A long acyclic chain (1→2→…→N) with a back-edge closing the whole thing into one
        // giant SCC — exercises the iterative frame stack on a deep graph (a recursive
        // Tarjan would risk overflow here, which is why the pass is iterative).
        let n = 5000u32;
        let mut edges: Vec<(u32, u32)> = (1..n).map(|i| (i, i + 1)).collect();
        edges.push((n, 1)); // close the cycle: {1..N} is one component
        let children_of = graph(&edges);
        let alloc: FxHashSet<Region> = (1..=n).map(Region).collect();
        let (comp_id, comp_members) = compute_sccs(&children_of, &alloc);
        // Every node lands in the one big component.
        let id = comp_id[&Region(1)];
        assert_eq!(comp_members[id].len(), n as usize);
        for i in 1..=n {
            assert_eq!(comp_id[&Region(i)], id);
        }
    }
}
