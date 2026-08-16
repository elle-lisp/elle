use super::*;

// ── The return frontier is per-path ───────────────────────────────────
//
// A returned region is the caller's to free only on the paths that hand it over.
// On a sibling arm that never uses the value no return mint fires, the caller
// receives nothing, and the callee still holds the only reference — so that arm
// owes a compensating release even though escape marks the region returnable.
// See docs/impl/region/mechanism.md § "The return frontier is per-path" and the
// end-to-end pin tests/elle/region-return-arm-escape-leak.lisp.

/// The body HirIds of the first `Match` in the tree, in arm order.
fn first_match_arms(hir: &Hir) -> Option<Vec<HirId>> {
    if let HirKind::Match { arms, .. } = &hir.kind {
        return Some(arms.iter().map(|(_p, _g, body)| body.id).collect());
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_match_arms(c);
        }
    });
    found
}

/// Does `arm` carry a compensating release for any region the binding named
/// `name` may point into?
fn arm_compensates(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
    arm: HirId,
) -> bool {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    info.branch_compensation
        .get(&arm)
        .is_some_and(|comp| regions.iter().any(|r| comp.contains(r)))
}

/// Does every region the binding named `name` may point into carry its release
/// OUTSIDE `arms` — i.e. at a node every arm reaches?
///
/// This is the branch-arm release window's signature (docs/impl/region/
/// mechanism.md § "A release inside one arm is not a release on the other
/// arms"): the region's one `decref_point` is re-anchored onto the branch, so no
/// arm holds it and none needs a compensating one.
fn release_clears_the_arms(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
    arms: &[HirId],
) -> bool {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    let order = compute_order(hir);
    let low = compute_subtree_low(hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    regions.iter().all(|r| match info.region_data.get(r) {
        Some(d) => {
            let o = ord(d.decref_point);
            !arms
                .iter()
                .any(|&a| low.get(&a).copied().unwrap_or(0) <= o && o <= ord(a))
        }
        None => false,
    })
}

// ── The obligation, and the two routes that discharge it ─────────────
//
// No path may leave a branch without releasing a region that was live-in to it.
// Two mechanisms discharge that one obligation, and the routing is a property of
// the region and the branch together. The window moves the region's single
// release to a point every arm reaches — admitted only where escape proves the
// frame is the region's sole holder. Where an arm leaves through a frame-replacing
// callee it reaches no merge, and the frame-exit relocation covers it instead, so
// such a branch narrows the window to the value-routed releases that relocation
// can replicate. Everything else keeps the in-arm release plus the per-arm
// compensation routes, which carry a count argument instead. The tests below pin
// each route on the shape that selects it.

#[test]
fn returned_param_is_released_on_the_arm_that_does_not_return_it() {
    // `(if (%eq i 0) xs 7)` — `xs` leaves through the THEN arm, so its
    // `decref_point` lands there. The ELSE arm hands the caller an immediate: no
    // mint, no caller reference, and nothing else releases the param. `xs` crosses
    // the return frontier, so the window declines it (its caller is a second
    // holder the anchor argument says nothing about) and compensation is the route.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs 7))");
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id),
        "the non-returning arm must release the returned param's region"
    );
}

#[test]
fn returned_param_compensation_follows_the_arms() {
    // The mirror: `xs` leaves through the ELSE arm, so the THEN arm owes the
    // release. Pins that the admission keys on which arm carries the value out,
    // not on arm position.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) 7 xs))");
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id),
        "the non-returning arm must release the returned param's region"
    );
}

#[test]
fn read_only_arm_release_clears_the_arms() {
    // Control: when no arm carries `xs` across the return frontier the same
    // anchoring applies. Guards against a change that treats the returned shape
    // specially by dropping the baseline.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) 7))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a merely-read param's release must sit where both arms reach it"
    );
}

#[test]
fn match_arms_are_treated_like_if_arms() {
    // Every premise here is stated over ONE ARM and its siblings — never over the
    // branch's arity or kind. `v` is allocated before the dispatch, so it is
    // live-in on every arm and no arm may hold its only release.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t) (let [v (list 1 2 3)] (match t :use (length v) :skip 0 _ -1)))",
    );
    let arms = first_match_arms(&hir).expect("a Match node");
    assert_eq!(arms.len(), 3, "the dispatch has three arms");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "v", &arms),
        "a Match's live-in local must be released where every arm reaches it"
    );
}

#[test]
fn a_frame_replacing_arm_anchors_a_value_routed_release() {
    // `(g 7)` in tail position replaces the frame, so the ELSE arm leaves through
    // the callee rather than arriving at the merge. The anchor alone does not
    // cover that arm — the frame-exit relocation replicates the anchored release
    // ahead of its `TailCall` — so the branch narrows to the releases that
    // relocation can replicate instead of declining whole
    // (docs/impl/region/mechanism.md § "An arm that leaves through a callee takes
    // a replica, not the anchor"). Only a VALUE route is replicable, which is what
    // the first assertion states about this shape and the second relies on.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) (g 7)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let b = find_binding_by_name(&hir, "xs", &arena, &symbols).expect("the param `xs`");
    assert!(
        info.binding_source_regions.get(&b).is_some_and(
            |rs| !rs.is_empty() && rs.iter().all(|r| info.call_result_regions.contains(r))
        ),
        "the narrowing admits only value-routed regions, so this pin needs `xs` \
         to be one — a region released by id keeps the whole-branch decline"
    );
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a frame-replacing sibling arm must not decline a value-routed release"
    );
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id),
        "the anchored release must not be doubled by a per-arm compensation"
    );
}

#[test]
fn a_native_tail_arm_does_not_decline_the_window() {
    // A tail call to a NATIVE pushes no frame and falls through to the merge, so
    // it is not a frame exit at all and the narrowing above never applies — the
    // distinction is the callee kind, not `is_tail`.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) (length xs)))");
    let (then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        release_clears_the_arms(&hir, &arena, &symbols, &info, "xs", &[then_id, else_id]),
        "a native tail call in an arm must not decline the window"
    );
}

#[test]
fn the_match_arm_that_uses_the_value_takes_no_head_compensation() {
    // The over-free counterfactual for the arm route: the arm holding the
    // `decref_point` already releases `v` at its own last use. A head release there
    // would precede that use and free the value under it.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (t) (let [v (list 1 2 3)] (match t :use (length v) :skip 0 _ -1)))",
    );
    let arms = first_match_arms(&hir).expect("a Match node");
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "v", arms[0]),
        "the arm that uses the local must not also take a head release"
    );
}

/// Does `node` carry a per-arm (`tail`) release for a region the binding named
/// `name` may point into?
fn arm_decrefs(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
    node: HirId,
) -> bool {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {}", name));
    let regions = match info.binding_source_regions.get(&b) {
        Some(rs) => rs,
        None => return false,
    };
    info.branch_arm_decrefs
        .get(&node)
        .is_some_and(|comp| regions.iter().any(|r| comp.contains(r)))
}

/// The HirIds of every `Return` node inside `[lo, hi]` of the post-order index.
fn returns_within(hir: &Hir, order: &HashMap<HirId, u32>, lo: u32, hi: u32) -> Vec<HirId> {
    find_all(hir, |h| matches!(&h.kind, HirKind::Return { .. }))
        .into_iter()
        .filter(|id| {
            let o = order.get(id).copied().unwrap_or(0);
            o >= lo && o <= hi
        })
        .collect()
}

#[test]
fn the_walk_base_case_releases_at_its_return() {
    // `(if (= i 0) xs (go (- i 1) xs))` — both arms use `xs`, so the recursive
    // arm's later use takes the `decref_point` and the base case is left with a
    // return mint and no release. The release belongs at the base case's `Return`,
    // where the mint has already raised the count.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(letrec [go (fn (i xs) (if (%eq i 0) xs (go (%sub i 1) xs)))] (go 1 (list 1 2)))",
    );
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    let order = compute_order(&hir);
    let lo = compute_subtree_low(&hir, &order)
        .get(&then_id)
        .copied()
        .expect("the then arm has a subtree interval");
    let hi = order
        .get(&then_id)
        .copied()
        .expect("the then arm is ordered");
    let rets = returns_within(&hir, &order, lo, hi);
    assert!(
        !rets.is_empty(),
        "the base-case arm must contain a Return node"
    );
    assert!(
        rets.iter()
            .any(|&n| arm_decrefs(&hir, &arena, &symbols, &info, "xs", n)),
        "the base case must release the arg at its Return, after the mint"
    );
}

// ── The env cell's compensating release ───────────────────────────────
//
// docs/impl/region/mechanism.md § "A compensating release of an env cell names the
// box, not the holder's slot". An env cell's box is minted once per activation and
// released through `LoadCaptureRaw` + `DecrefCellRegion`, which names the box
// rather than the holder's slot. Every refusal that would decline a per-arm release
// of it — a reassigned holder, a capturer's alias, the return frontier, the
// same-node retain — is a claim about the VALUE that holder names, so both routes
// carry the box: `head` where the arm names the binding nowhere, `tail` after that
// arm's last use where it reads it.

/// The env-cell region of the binding named `name` — the `cell_release_regions`
/// member among its source regions.
fn env_cell_region(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    info: &RegionInfo,
    name: &str,
) -> Region {
    let b = find_binding_by_name(hir, name, arena, symbols)
        .unwrap_or_else(|| panic!("no binding named {name}"));
    info.binding_source_regions
        .get(&b)
        .into_iter()
        .flatten()
        .copied()
        .find(|r| info.cell_release_regions.contains(r))
        .unwrap_or_else(|| {
            panic!(
                "binding `{name}` must hold an env-cell region; source={:?} cell_release={:?}",
                info.binding_source_regions.get(&b),
                info.cell_release_regions,
            )
        })
}

#[test]
fn a_falling_through_arm_compensates_the_env_cell_its_sibling_relocated() {
    // `(if t (g) 0)` — `g` captures the in-lambda mutable `c`, so the box is an env
    // cell whose one `DecrefCellRegion` sits in the THEN arm (the frame-exit
    // relocation moves it ahead of that arm's `TailCall`). The ELSE arm reaches the
    // merge and names `c` nowhere, so it is a dead sibling arm and owes the head
    // release; without it the box strands once per call.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (n t) (def @c n) (let [g (fn () c)] (if t (g) 0)))");
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cell = env_cell_region(&hir, &arena, &symbols, &info, "c");
    assert!(
        info.branch_compensation
            .get(&else_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "the falling-through arm must release the env cell r{}; branch_compensation={:?}",
        cell.0,
        info.branch_compensation,
    );
}

#[test]
fn a_reassigned_holder_does_not_withdraw_its_env_cell_compensation() {
    // The mutated face. A reassigned holder poisons a release routed through its
    // SLOT, and this release names the box no `assign` repoints — so the same head
    // release is owed. Pins that the refusal is read per region, not per holder.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(fn (n t) (def @c n) (let [g (fn () (assign c (%add c 1)) c)] (if t (g) 0)))",
    );
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    let cell = env_cell_region(&hir, &arena, &symbols, &info, "c");
    assert!(
        info.branch_compensation
            .get(&else_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "a reassigned holder's env cell r{} must still take the head release; \
         branch_compensation={:?}",
        cell.0,
        info.branch_compensation,
    );
}

#[test]
fn an_env_cell_takes_the_tail_route_on_the_arm_that_reads_it() {
    // `(if t c (g))` — the capture-use of `c` resolves through `g`'s last use, so
    // the box's `decref_point` follows the CALL and lands in the else arm. The then
    // arm READS `c`, making it a used sibling arm: the head route fires before the
    // arm's body and would free the box under that read. The `tail` route releases
    // after the read instead, and needs no same-node retain — the box's holders are
    // the frame's env slot plus one counted `closure ⊇ cell` edge per capturer, and
    // no use of the binding yields the box.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (n t) (def @c n) (let [g (fn () c)] (if t c (g))))");
    let cell = env_cell_region(&hir, &arena, &symbols, &info, "c");
    assert!(
        info.branch_arm_decrefs
            .values()
            .any(|rs| rs.contains(&cell)),
        "the arm that reads the cell's binding must release the box r{} after that \
         read; branch_arm_decrefs={:?}",
        cell.0,
        info.branch_arm_decrefs,
    );
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !info
            .branch_compensation
            .get(&then_id)
            .is_some_and(|comp| comp.contains(&cell)),
        "the reading arm must take no head release of r{} — that frees the box \
         under its own read",
        cell.0,
    );
}

#[test]
fn an_unfunded_used_sibling_arm_takes_no_tail_route() {
    // The counterfactual that keeps the retain requirement on every other region.
    // Both arms name `xs` and neither node retains it — no store, no `-mut`
    // container, no return mint — so the arm that loses the `decref_point` max keeps
    // the conservative baseline. Only a cell release is admitted without one.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (t xs) (let [n (if t (length xs) (length xs))] (%add n 1)))");
    let b = find_binding_by_name(&hir, "xs", &arena, &symbols).expect("the param `xs`");
    let regions: Vec<Region> = info
        .binding_source_regions
        .get(&b)
        .cloned()
        .unwrap_or_default();
    assert!(!regions.is_empty(), "`xs` must hold a region to judge");
    assert!(
        info.branch_arm_decrefs
            .values()
            .all(|rs| regions.iter().all(|r| !rs.contains(r))),
        "an unfunded used sibling arm must take no `tail` release; \
         regions={regions:?} branch_arm_decrefs={:?}",
        info.branch_arm_decrefs,
    );
}

#[test]
fn the_arm_that_returns_the_value_takes_no_compensation() {
    // The over-free counterfactual: the arm that DOES hand the value to the
    // caller must be left alone — its `decref_point` release, paired with the
    // mint, is the whole hand-over. A compensating release there would take the
    // caller's reference.
    let (hir, arena, symbols, info) = analyze_with_class("(fn (i xs) (if (%eq i 0) xs 7))");
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        !arm_compensates(&hir, &arena, &symbols, &info, "xs", then_id),
        "the returning arm must not also compensate — that frees the caller's value"
    );
}
