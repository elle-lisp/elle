// audited: 2026-10-20
//! Each Tarjan component equals the brute-force mutual-reachability set, over a battery of small graphs.
//!
//! docs/impl/region/ownership.md
//!
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
