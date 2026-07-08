use super::*;

// ── ownership inference: externally-unique Owned subtrees (step 2) ─────────
//
// `regions::ownership::compute_owned_subtrees` consumes the Shared-seed set and walks
// the region containment graph (`cross_region_refs` plus the funnel-recovered
// `containment_edges`, target ⊇ source) outward from each
// candidate root, reporting the subtrees that free as a unit: no interior region crosses
// a frontier (none is a Shared seed) and nothing outside references inside. These pins
// are written from that definition. The positives claim the discarded builder idiom (the
// multi-region generalization of the builder-idiom MERGE) and a lone non-escaping
// allocation; the negatives are counterfactuals each proven to bite a deliberately-greedy
// walk — a returned root (seed), an escaping interior child (interior seed), region-level
// aliasing (outside-references-in), and a may-store clique (a hard edge defeats
// uniqueness). The map is computed, not yet consumed.

#[test]
fn owned_subtree_claims_discarded_nested_literal() {
    // The builder idiom, discarded: a fresh inner pair stored as the car of a fresh
    // outer pair, the whole literal thrown away (the begin's non-tail). The outer pair
    // is an Owned root whose subtree is {outer, inner} — the multi-region generalization
    // of the builder-idiom MERGE. Counterfactual: an empty (un-implemented) walk reports no root.
    let (hir, info, owned) = owned_subtrees("(begin (%pair (%pair 1 2) 3) nil)");
    let edges = pair_store_edges(&hir, &info);
    assert_eq!(
        edges.len(),
        1,
        "exactly one child→parent store edge; got {:?}",
        edges
    );
    let (child, parent) = edges[0];
    let subtree = owned.get(&parent).unwrap_or_else(|| {
        panic!(
            "the outer pair r{} must be an Owned root; owned={:?}",
            parent.0, owned
        )
    });
    assert!(
        subtree.contains(&parent) && subtree.contains(&child),
        "the Owned subtree at r{} must contain both the parent and the interior child \
         r{}; got {:?}",
        parent.0,
        child.0,
        subtree,
    );
    assert!(
        !owned.contains_key(&child),
        "the interior child r{} is not itself a root — it is contained in r{}; owned={:?}",
        child.0,
        parent.0,
        owned,
    );
}

#[test]
fn owned_subtree_claims_lone_local_singleton() {
    // A pair built, read locally (`%first`), and discarded — it crosses no frontier and
    // has no containment edges, so it is a singleton Owned subtree {p}. (Subtree drop of
    // a singleton is the unmerged baseline, but it pins that a purely-local allocation is
    // an Owned candidate, not Shared.)
    let (hir, info, owned) = owned_subtrees("(begin (let [p (%pair 1 2)] (%first p)) nil)");
    let p = sole_pair_region(&hir, &info);
    let subtree = owned.get(&p).unwrap_or_else(|| {
        panic!(
            "the lone local pair r{} must be an Owned root; owned={:?}",
            p.0, owned
        )
    });
    assert_eq!(
        subtree.len(),
        1,
        "a lone local pair's subtree is the singleton {{r{}}}; got {:?}",
        p.0,
        subtree
    );
    assert!(
        subtree.contains(&p),
        "the singleton subtree must contain r{}",
        p.0
    );
}

#[test]
fn owned_subtree_refuses_returned_root() {
    // `p` is returned (the program's tail) → a Shared seed → never Owned.
    let (hir, info, owned) = owned_subtrees("(let [p (%pair 1 2)] p)");
    let p = sole_pair_region(&hir, &info);
    assert!(
        !in_some_owned_subtree(&owned, p),
        "a returned region r{} must not be claimed Owned (it crosses the return \
         frontier); owned={:?}",
        p.0,
        owned,
    );
}

#[test]
fn owned_subtree_refuses_escaping_interior_child() {
    // The interior child `inner` is stored into the discarded outer pair AND returned.
    // The outer pair is itself discarded, so a per-root escape check would wrongly own
    // it; the interior-seed check refuses it because its subtree contains the returned
    // child (a Shared seed). Both regions must be refused.
    let (hir, info, owned) =
        owned_subtrees("(let [inner (%pair 1 2)] (begin (%first (%pair inner 8)) inner))");
    let edges = pair_store_edges(&hir, &info);
    assert!(
        !edges.is_empty(),
        "precondition: a child→parent pair-store edge must exist"
    );
    for (child, parent) in edges {
        assert!(
            !in_some_owned_subtree(&owned, parent),
            "the outer pair r{} must not be Owned — its subtree contains the returned \
             interior child r{} (a Shared seed); owned={:?}",
            parent.0,
            child.0,
            owned,
        );
        assert!(
            !in_some_owned_subtree(&owned, child),
            "the returned interior child r{} must not be Owned; owned={:?}",
            child.0,
            owned,
        );
    }
}

#[test]
fn owned_subtree_refuses_region_level_alias() {
    // `inner` is stored into TWO distinct parent pairs. Rooting at either parent, the
    // other parent's reference to `inner` is an edge from outside into the subtree, so
    // external uniqueness fails for both. Counterfactual against a walk missing the
    // outside-references-in check.
    let (hir, info, owned) = owned_subtrees(
        "(let [inner (%pair 1 2)] (begin (%first (%pair inner 8)) (%first (%pair inner 9))))",
    );
    let edges = pair_store_edges(&hir, &info);
    let child = edges
        .iter()
        .find(|(c, _)| edges.iter().filter(|(c2, _)| c2 == c).count() >= 2)
        .map(|&(c, _)| c)
        .expect("precondition: a child stored into two distinct parents");
    let parents: Vec<Region> = edges
        .iter()
        .filter(|(c, _)| *c == child)
        .map(|&(_, p)| p)
        .collect();
    assert!(parents.len() >= 2, "precondition: two distinct parents");
    for p in parents {
        assert!(
            !in_some_owned_subtree(&owned, p),
            "parent r{} must not be Owned — the aliased child r{} is also referenced from \
             the other parent, outside this subtree; owned={:?}",
            p.0,
            child.0,
            owned,
        );
    }
}

#[test]
fn owned_subtree_refuses_may_store_clique() {
    // `has?` is declared `Mixed` (a trait-dispatched native), so it records a may-store
    // clique between its two heap (string-literal) args. A clique edge is a may-store, so
    // it builds no subtree (ineligible) — but it DOES defeat external uniqueness: rooting
    // at one arg's region, the clique edge to the other arg is a reference from outside
    // into the subtree. The result is discarded (`begin … nil`) so neither arg is a return
    // seed — the refusal is the hard-edge check alone. Counterfactual against a walk that
    // ignores hard edges in the outside-references-in check. Uses the REAL classification
    // (`has?` genuinely Mixed), not a forced effect.
    let (hir, info, owned) = owned_subtrees_with_effects("(begin (has? \"a\" \"b\") nil)");
    let r_a = string_literal_region(&hir, &info, "a");
    let r_b = string_literal_region(&hir, &info, "b");
    assert!(
        !in_some_owned_subtree(&owned, r_a) && !in_some_owned_subtree(&owned, r_b),
        "may-store clique args r{}/r{} must not be Owned (each is referenced from the \
         other through a hard edge); owned={:?}",
        r_a.0,
        r_b.0,
        owned,
    );
}

#[test]
fn owned_subtree_unifies_local_capture_into_closure() {
    // A pair `p` captured by a LOCAL closure `c` (called in place, discarded) must be
    // INTERIOR to `c`'s Owned subtree, not an independent Owned root. Capture records no
    // `cross_region_refs` edge (the RC double-count fix), so without the re-derived
    // capture edge `p` is wrongly claimed its own singleton subtree while `c`'s env still
    // references it — a double-free at emit. The assertion needs no handle on `c`'s
    // region: `p` must be non-root (`!contains_key`) yet present in some subtree.
    // Counterfactual: the pre-capture-edge walk makes `p` its own root → `contains_key`
    // is true → RED.
    let (hir, info, owned) =
        owned_subtrees("(begin (let [p (%pair 1 2)] (let [c (fn [] (length p))] (c))) nil)");
    let p = sole_pair_region(&hir, &info);
    assert!(
        !owned.contains_key(&p),
        "the captured pair r{} must not be its own Owned root — it is interior to the \
         capturing closure's subtree; owned={:?}",
        p.0,
        owned,
    );
    assert!(
        in_some_owned_subtree(&owned, p),
        "the captured pair r{} must be a member of the local closure's Owned subtree; \
         owned={:?}",
        p.0,
        owned,
    );
}

#[test]
fn owned_subtree_refuses_captured_by_escaping_closure() {
    // `p` is captured by a closure that ESCAPES (it is the program's return value). The
    // closure's region is a Shared seed (returned), so the closure is not an Owned root,
    // and `p` — contained in it by the re-derived capture edge — is reachable only
    // through that escaping container, hence Shared (not Owned). This is the
    // `shared_seed_excludes_captured_but_not_returned` shape viewed through ownership: `p`
    // is not a *seed* but is *not Owned* either. Counterfactual: the pre-capture-edge
    // walk has no edge from `p`, so `p` is a sole-held top container → wrongly claimed an
    // Owned singleton → RED.
    let (hir, info, owned) = owned_subtrees("(let [p (%pair 1 2)] (fn [] (length p)))");
    let p = sole_pair_region(&hir, &info);
    assert!(
        !in_some_owned_subtree(&owned, p),
        "a value r{} captured by an escaping closure must not be Owned (its container \
         crosses the return frontier); owned={:?}",
        p.0,
        owned,
    );
}

// ── ownership inference: co-owned-cycle groups (compute_owned_region_groups) ──────
//
// A mutual reference cycle with no container parent has no owner among its members, so
// `compute_owned_subtrees` (which roots only at top containers) refuses it. The
// co-owned-cycle cut claims it as one symmetric group, reclaimed wholesale at its
// collective last use. These pins are written from that definition.

#[test]
fn owned_region_groups_claims_bare_cycle() {
    // The bare cycle: two `@array` call-results push each other (a ⊇ b, b ⊇ a), the
    // whole thing discarded. No top container holds either, so compute_owned_subtrees
    // refuses it (pinned by the empty-owned assertion below); the co-owned-cycle cut
    // claims {a, b} as one group. Counterfactual: a walk that does not promote a rootless
    // source SCC returns no group — RED before the cut.
    let src = "(begin (let [a (@array) b (@array)] \
                        (begin (%array-push a b) (%array-push b a) nil)) nil)";
    let (_, info, groups) = owned_region_groups(src);
    // The two cycle members are the endpoints of the funnel-recovered containment
    // edges (the pushes are opaque funnel calls — no cross_region_refs edge).
    let mut members = rustc_hash::FxHashSet::default();
    for &(_site, src_r, dst_r) in &info.containment_edges {
        members.insert(src_r);
        members.insert(dst_r);
    }
    assert_eq!(
        members.len(),
        2,
        "precondition: the bare cycle has exactly two members; got {:?}",
        members
    );
    // compute_owned_subtrees must NOT claim either member (neither is a top container) —
    // the case that motivates the co-owned-cycle cut.
    let (_, _, owned) = owned_subtrees_with_effects(src);
    for &m in &members {
        assert!(
            !in_some_owned_subtree(&owned, m),
            "member r{} must not be a container-rooted Owned subtree — the bare cycle has \
             no top container; owned={:?}",
            m.0,
            owned,
        );
    }
    // Exactly one co-owned group, containing exactly both members.
    assert_eq!(
        groups.len(),
        1,
        "the bare cycle is exactly one co-owned group; got {:?}",
        groups
    );
    let group: rustc_hash::FxHashSet<Region> =
        groups.values().next().unwrap().iter().copied().collect();
    assert_eq!(
        group, members,
        "the co-owned group must be exactly the two cycle members; got {:?}",
        group
    );
}

#[test]
fn owned_region_groups_refuses_container_rooted_shape() {
    // Soundness boundary: a container-rooted shape (a fresh container holding a value,
    // no cycle) is NOT a co-owned group — it is compute_owned_subtrees' territory. The
    // group walk must leave it empty (else it would double-claim a region that the adopt
    // path already owns). Counterfactual against a walk that treats any owned structure
    // as a group.
    let (_, _, groups) = owned_region_groups("(begin (%array-push (@array) (array 1 2)) nil)");
    assert!(
        groups.is_empty(),
        "a container-rooted (acyclic) shape must yield no co-owned group; got {:?}",
        groups,
    );
}

#[test]
fn owned_region_groups_drop_site_post_dominates_pass_through_aliases() {
    // Drop-site post-dominance counterfactual. A `%array-push`
    // is pass-through: it returns its container, so the discarded store RESULT
    // (`alloc_region[store_site]`) is an alias of a member whose `result_region_of` deref
    // lands one structural step PAST the member's own `decref_point`. The group's drop site
    // must post-dominate every such release, or `FreeRegionGroup` frees the member before
    // the alias deref — the stale-deref UAF. The previous drop site (`max(member
    // decref_point)`) falls strictly before those releases, so this assertion bites it; the
    // enclosing-scope drop site (a structural ancestor of all members) post-dominates them.
    let src = "(begin (let [a (@array) b (@array)] \
                        (begin (%array-push a b) (%array-push b a) nil)) nil)";
    let (hir, info, groups) = owned_region_groups(src);
    assert_eq!(
        groups.len(),
        1,
        "the bare cycle is exactly one group; got {:?}",
        groups
    );
    let (&drop_site, members) = groups.iter().next().unwrap();
    let member_set: rustc_hash::FxHashSet<Region> = members.iter().copied().collect();
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let mut checked = 0;
    for &(site, src_r, dst_r) in &info.containment_edges {
        if !(member_set.contains(&src_r) && member_set.contains(&dst_r)) {
            continue;
        }
        let result_region = info
            .alloc_region
            .get(&site)
            .copied()
            .expect("a pass-through push store site has a call-result region");
        let dp = info
            .region_data
            .get(&result_region)
            .expect("the store result has a decref_point")
            .decref_point;
        assert!(
            ord(dp) <= ord(drop_site),
            "the pass-through result of within-cycle store @{} dies at order {}, AFTER the \
             group drop site (order {}) — freeing the group there is the \
             stale-deref UAF",
            site.0,
            ord(dp),
            ord(drop_site),
        );
        checked += 1;
    }
    assert!(
        checked >= 2,
        "precondition: both within-cycle push stores must be checked; got {checked}",
    );
}

#[test]
fn owned_subtree_claims_mutable_container_over_call_result() {
    // The production-path shape (and the scheduler's mutable cell in miniature): a
    // mutable container and its stored value are both `Fresh` native CALL results —
    // bare `@array`/`array` are ordinary calls, so their regions are `call_result`
    // placeholders, not static allocations. A `Fresh` call-result is nonetheless a
    // genuinely caller-owned fresh allocation, so it must be an Owned candidate, not
    // refused like a pass-through/opaque call-result borrow. `%array-push` is an
    // opaque `Funnel` CALL that records NO `cross_region_refs` edge (the funnel
    // counts the store at runtime — a compile-time edge would double-count); the
    // alloc-type recovery re-supplies the containment edge `value -> container` from
    // the container's RetType (MutableArray), so the container is an Owned root
    // whose subtree includes the funnel-stored value.
    //
    // Counterfactuals: before `Fresh` call-results were admitted as ownable, both
    // regions were refused (`call_result_regions`) and the root lookup panics;
    // without the alloc-type recovery, `containment_edges` is empty (the Funnel
    // store records nothing), so the container is at most a singleton and the value
    // is not in its subtree. The real primitive classification is required so
    // `@array`/`array` resolve to their declared `Fresh` effect and `%array-push` to
    // `Funnel` (the default empty classification treats them as opaque user fns).
    let (_, info, owned) =
        owned_subtrees_with_effects("(begin (%array-push (@array) (array 1 2)) nil)");
    assert!(
        info.cross_region_refs.is_empty(),
        "precondition: the Funnel store records no cross_region_refs edge; got {:?}",
        info.cross_region_refs
    );
    assert_eq!(
        info.containment_edges.len(),
        1,
        "the alloc-type recovery records exactly one containment edge \
         (value -> container); got {:?}",
        info.containment_edges
    );
    let (_site, value, container) = info.containment_edges[0];
    let subtree = owned.get(&container).unwrap_or_else(|| {
        panic!(
            "the mutable container r{} (a Fresh call-result) must be an Owned root; \
             owned={:?}",
            container.0, owned
        )
    });
    assert!(
        subtree.contains(&container) && subtree.contains(&value),
        "the Owned subtree at r{} must contain the container and the funnel-stored \
         value r{}; got {:?}",
        container.0,
        value.0,
        subtree,
    );
}

#[test]
fn owned_subtree_no_funnel_containment_for_immutable_container() {
    // Soundness gate (counterfactual): pushing into an IMMUTABLE `array` returns a
    // fresh copy — arg0 does NOT gain the value (arg0 ⊉ value). `array` is RetType
    // Array, not MutableArray, so it is not a mutable retaining container and the
    // recovery records NO containment edge. Recording `arg0 ⊇ value` here would be
    // a use-after-free (subtree-dropping the immutable array would free a value it
    // never retained), so the mutable-only gate is load-bearing, not an optimization.
    let (_, info, _owned) =
        owned_subtrees_with_effects("(begin (%array-push (array 1 2) (array 3 4)) nil)");
    assert!(
        info.containment_edges.is_empty(),
        "an immutable-array funnel store must record no containment edge (arg0 \
         returns fresh, retains nothing); got {:?}",
        info.containment_edges
    );
}
