// audited: 2026-11-09
//! The root's lifetime obligation on captured and stored members: admitted when the root dies last, refused when a member outlives it.
//!
//! docs/impl/region/adopt.md
//! docs/impl/region/owner.md
//!
//! Each refusal sits beside the shape it admits. A binder's init is its own last use
//! (docs/impl/region/lastuse.md), so a captured value dies with its closure's last
//! call, and the admitting shapes are the ones a release held a step past the closure
//! used to refuse.

use super::*;

/// The pair, closure and closure of a nested capture `c1 ⊇ c2 ⊇ p`.
const NESTED: &str = "(let [p (%pair 1 2)] \
                        (let [c2 (fn [] (%first p))] \
                          (let [c1 (fn [] (c2))] BODY)))";

/// A closure `c1` capturing a closure `c2` capturing a pair `p`, all discarded when
/// `(c1)` returns. The subtree is rooted at `c1`, which adopts `c2`, which adopts `p`.
///
/// Counter-factual: a tight last use that cascades one structural step per capture
/// level puts `p` past `c1`'s drop, and the obligation refuses the chain to Shared.
#[test]
fn adopt_edges_claims_nested_capture_chain() {
    let src = format!("(begin {} nil)", NESTED.replace("BODY", "(c1)"));
    let (hir, info, edges) = adopt_edges(&src);
    let p = sole_pair_region(&hir, &info);
    let adopts: Vec<(Region, Region)> = edges.capture.values().flatten().copied().collect();
    let owner_of = |m: Region| {
        adopts
            .iter()
            .find(|&&(x, _)| x == m)
            .map(|&(_, o)| o)
            .unwrap_or_else(|| panic!("r{} is adopted; got {adopts:?}", m.0))
    };
    let c2 = owner_of(p);
    let c1 = owner_of(c2);
    assert!(
        c1 != c2 && adopts.len() == 2 && edges.store.is_empty(),
        "the chain adopts p by c2 and c2 by c1, by capture alone; got capture={adopts:?} \
         store={:?}",
        edges.store,
    );
    for (member, closure) in adopts {
        assert!(
            closure_captures_region(&hir, &info, member, closure),
            "the adopt of r{} by r{} has no capture to reload it from",
            member.0,
            closure.0,
        );
    }
}

/// The same chain with `p` read again after `(c1)` returns. `p` outlives the root,
/// so the obligation refuses the externally-unique subtree and nothing is adopted.
///
/// Counter-factual: a cut that capped a captured value at its closure's death,
/// whatever reads it later, would free `p` under the trailing `(%first p)`.
#[test]
fn adopt_edges_refuses_nested_capture_read_after_the_root() {
    let src = format!(
        "(begin {} nil)",
        NESTED.replace("BODY", "(begin (c1) (%first p))")
    );
    let (hir, info, edges) = adopt_edges(&src);
    let p = sole_pair_region(&hir, &info);
    // The refusal is the obligation's, not the external-uniqueness walk's.
    let (_, _, owned) = owned_subtrees_with_effects(&src);
    let root = owned
        .iter()
        .find(|(_, members)| members.contains(&p))
        .map(|(&r, _)| r)
        .expect("the subtree {c1, c2, p} is externally unique");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info.region_data[&root].decref_point);
    let p_tight = ord(info.binding_last_use[&p]);
    assert!(
        p_tight > root_dp,
        "precondition: the pair r{}'s tight last use ({p_tight}) falls after the root \
         r{}'s release ({root_dp})",
        p.0,
        root.0,
    );
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "a member read after the root dies refuses the subtree; got store={:?} \
         capture={:?}",
        edges.store,
        edges.capture,
    );
}

/// `root ⊇ m` and `m ⊇ c` by store, `c ⊇ m` by capture: the m↔c cycle under a root.
/// `BODY` orders the call of `c` against the root's last store.
const BACK_EDGE: &str = "(begin (let [root (@array) m (@array)] \
                           (let [c (fn [] (length m))] (begin (%array-push m c) BODY nil))) \
                         nil)";

/// The containers of the back-edge shape: `(root, m)`. `m` is both a store source and a
/// store target among the funnel containment edges, and `root` is a target only.
fn root_and_m(info: &RegionInfo) -> (Region, Region) {
    let srcs: rustc_hash::FxHashSet<Region> =
        info.containment_edges.iter().map(|&(_, s, _)| s).collect();
    let dsts: rustc_hash::FxHashSet<Region> =
        info.containment_edges.iter().map(|&(_, _, d)| d).collect();
    let m = *srcs
        .iter()
        .find(|r| dsts.contains(r))
        .expect("m is a source and a target");
    let root = *dsts
        .iter()
        .find(|r| !srcs.contains(r))
        .expect("a target-only root");
    (root, m)
}

/// `c` runs before the root's last store, so the root's drop follows every use of `m`
/// and `c`. The root owns the cycle: it adopts `m`, and `m` adopts `c`, each by store.
/// The capture back-edge carries no adopt and needs none.
///
/// Counter-factual: with `m`'s release held a step past its closure, `m`'s own live
/// `DecrefValueRegion` fell after the root's drop, and the obligation refused the
/// subtree. The runtime reclaims the cycle under its root
/// (`runtime::tests::ownership::rooted::region_ownership_capture_back_edge_cycle_reclaims_under_its_root`).
#[test]
fn adopt_edges_claims_captured_store_member_dead_before_the_root() {
    let src = BACK_EDGE.replace("BODY", "(c) (%array-push root m)");
    let (hir, info, edges) = adopt_edges(&src);
    let (root, m) = root_and_m(&info);
    let c = sole_closure_region(&hir, &info);
    let store: rustc_hash::FxHashSet<(Region, Region)> =
        edges.store.values().flatten().copied().collect();
    assert_eq!(
        store,
        [(m, root), (c, m)].into_iter().collect(),
        "the root r{} adopts m r{}, and m adopts c r{}; got store={store:?}",
        root.0,
        m.0,
        c.0,
    );
    assert!(
        edges.capture.is_empty() && info.activation_adopt_sites.is_empty(),
        "a region root owns the cycle, so no capture adopt and no activation site; got \
         capture={:?} activation={:?}",
        edges.capture,
        info.activation_adopt_sites,
    );
}

/// `c` runs after the root's last store, so `m`, which `c` reads, outlives the root.
/// `m` is store-adopted, so its own `DecrefValueRegion` stays live, and the root's drop
/// would free `m` under it. The obligation reads `m`'s structural release, refuses the
/// subtree, and the activation cut owns the cycle instead
/// (`cuts::activation_adopts_capture_back_edge_scc`).
///
/// Counter-factual: an obligation that bounded a store-adopted member by its tight
/// last use alone would admit the subtree and free `m` before its own release.
#[test]
fn adopt_edges_refuses_captured_store_member_on_lifetime() {
    let src = BACK_EDGE.replace("BODY", "(%array-push root m) (c)");
    let (hir, info, edges) = adopt_edges(&src);
    let (root, m) = root_and_m(&info);
    let (_, _, owned) = owned_subtrees_with_effects(&src);
    assert!(
        owned.values().any(|s| s.contains(&m) && s.contains(&root)),
        "the refusal is the obligation's: {{root, m, c}} is externally unique; got \
         {owned:?}",
    );
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let root_dp = ord(info.region_data[&root].decref_point);
    let m_dp = ord(info.region_data[&m].decref_point);
    assert!(
        m_dp > root_dp,
        "precondition: m r{}'s own release ({m_dp}) falls after the root r{}'s \
         ({root_dp})",
        m.0,
        root.0,
    );
    assert!(
        edges.store.is_empty() && edges.capture.is_empty(),
        "a store-adopted member released after the root refuses the subtree; got \
         store={:?} capture={:?}",
        edges.store,
        edges.capture,
    );
}
