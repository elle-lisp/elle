use super::*;

// ── A call's result is named by the call's own region ─────────────────
//
// The walk inlines a resolvable lambda callee's body so the intrinsics inside it
// record their edges at this call site. The regions that walk yields are the
// CALLEE's — minted against the callee's own nodes, remapped per activation — so
// they must not become the caller's naming for the result. See
// docs/impl/region/mechanism.md § "A call's result is named by the call's own
// region" and the end-to-end pin tests/elle/region-inline-result-naming.lisp.

/// The HirId of the first `Call` whose callee names `callee`, looking through the
/// `DerefCell` wrapper functionalization puts around a `needs_capture` read.
fn call_to(hir: &Hir, callee: &str, arena: &BindingArena, symbols: &SymbolTable) -> Option<HirId> {
    fn names(func: &Hir, callee: &str, arena: &BindingArena, symbols: &SymbolTable) -> bool {
        match &func.kind {
            HirKind::Var(b) => symbols.name(arena.get(*b).name) == Some(callee),
            HirKind::DerefCell { cell } => names(cell, callee, arena, symbols),
            _ => false,
        }
    }
    fn walk(
        hir: &Hir,
        callee: &str,
        arena: &BindingArena,
        symbols: &SymbolTable,
        found: &mut Option<HirId>,
    ) {
        if found.is_none() {
            if let HirKind::Call { func, .. } = &hir.kind {
                if names(func, callee, arena, symbols) {
                    *found = Some(hir.id);
                }
            }
        }
        hir.for_each_child(|c| walk(c, callee, arena, symbols, found));
    }
    let mut found = None;
    walk(hir, callee, arena, symbols, &mut found);
    found
}

/// How many bindings are recorded as holders of `r` — the count
/// `regions::compensate`'s single-holder value route reads.
fn holder_count(info: &RegionInfo, r: Region) -> usize {
    info.binding_source_regions
        .values()
        .filter(|rs| rs.contains(&r))
        .count()
}

#[test]
fn an_inlined_call_result_is_named_by_the_call_node() {
    // `mk` binds a lambda this unit can see, so the walk inlines its body. The
    // binding that holds the result must still name the CALL's own region — the
    // caller-side marker the value-resolved release resolves at runtime — and not
    // the region `mk`'s body minted for `(list 1 2)`.
    let (hir, arena, symbols, info) =
        analyze_with_class("(let [mk (fn () (list 1 2))] (let [v (mk)] (length v)))");
    let call = call_to(&hir, "mk", &arena, &symbols).expect("a call to mk");
    let call_r = info
        .alloc_region
        .get(&call)
        .copied()
        .expect("the call node has its own region");
    let v = find_binding_by_name(&hir, "v", &arena, &symbols).expect("a binding named v");
    assert_eq!(
        info.binding_source_regions.get(&v),
        Some(&vec![call_r]),
        "the result binding must name the call's own region, not the callee's"
    );
}

#[test]
fn a_base_arms_result_is_released_in_the_base_arm() {
    // The shape the naming leak bites: the recursive arm inlines the same body, so
    // without the rule its result binding names the BASE arm's result region too —
    // and, being structurally later, takes that region's one release into an arm
    // mutually exclusive with the only path that mints it.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(letrec [go (fn (m) (if (%eq m 0) (list 1 2) (go (%sub m 1))))] (go 1))",
    );
    let base = call_to(&hir, "list", &arena, &symbols).expect("a call to list");
    let r = info
        .alloc_region
        .get(&base)
        .copied()
        .expect("the base arm's result has a region");
    let (then_id, _else_id) = first_if_arms(&hir).expect("an If node");
    let order = compute_order(&hir);
    let low = compute_subtree_low(&hir, &order);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let d = ord(info
        .region_data
        .get(&r)
        .expect("the base arm's region has a decref point")
        .decref_point);
    let (lo, hi) = (low.get(&then_id).copied().unwrap_or(0), ord(then_id));
    assert!(
        lo <= d && d <= hi,
        "the base arm's own result must be released in that arm, not in its sibling"
    );
}

#[test]
fn an_inlined_result_region_keeps_one_holder() {
    // The second consequence of adopting the callee's naming: the region gains a
    // holder binding in the sibling arm, and a two-holder region is refused the
    // single-slot value route `regions::compensate` needs to place a per-arm
    // release — so the arm the misplacement stranded cannot be compensated either.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(letrec [go (fn (m) (if (%eq m 0) (list 1 2) (go (%sub m 1))))] (go 1))",
    );
    let base = call_to(&hir, "list", &arena, &symbols).expect("a call to list");
    let r = info
        .alloc_region
        .get(&base)
        .copied()
        .expect("the base arm's result has a region");
    assert_eq!(
        holder_count(&info, r),
        1,
        "a call result must be held by the binding that names that call alone"
    );
}
