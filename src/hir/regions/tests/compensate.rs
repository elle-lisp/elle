use super::*;

// ── The return frontier is per-path ───────────────────────────────────
//
// A returned region is the caller's to free only on the paths that hand it over.
// On a sibling arm that never uses the value no return mint fires, the caller
// receives nothing, and the callee still holds the only reference — so that arm
// owes a compensating release even though escape marks the region returnable.
// See docs/impl/region/mechanism.md § "The return frontier is per-path" and the
// end-to-end pin tests/elle/region-return-arm-escape-leak.lisp.

/// The `(then_id, else_id)` body HirIds of the first `If` in the tree.
fn first_if_arms(hir: &Hir) -> Option<(HirId, HirId)> {
    if let HirKind::If {
        then_branch,
        else_branch,
        ..
    } = &hir.kind
    {
        return Some((then_branch.id, else_branch.id));
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_if_arms(c);
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

#[test]
fn returned_param_is_released_on_the_arm_that_does_not_return_it() {
    // `(if (%eq i 0) xs 7)` — `xs` leaves through the THEN arm, so its
    // `decref_point` lands there. The ELSE arm hands the caller an immediate: no
    // mint, no caller reference, and nothing else releases the param.
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
fn read_only_arm_keeps_its_compensation() {
    // Control: when no arm carries `xs` across the return frontier the ordinary
    // dead-arm compensation already applies. Guards against a change that admits
    // the returned shape by dropping the baseline.
    let (hir, arena, symbols, info) =
        analyze_with_class("(fn (i xs) (if (%eq i 0) (length xs) 7))");
    let (_then_id, else_id) = first_if_arms(&hir).expect("an If node");
    assert!(
        arm_compensates(&hir, &arena, &symbols, &info, "xs", else_id),
        "the dead sibling arm of a merely-read value must still be compensated"
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
