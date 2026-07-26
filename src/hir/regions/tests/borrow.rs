use super::*;

// ── The container-read BORROW ────────────────────────────────────────────────────
//
// A value read out of a container with `get`/`first`/`rest` still lives INSIDE that
// container — a pair's car in the pair's own region, an `@array` element in the member
// region the container holds. What keeps that borrow alive splits by read form, so the
// walk records the two separately:
//
//   - An OPCODE read (`%get`/`%first`/`%rest`) raises no count at all, so the container's
//     own lifetime is the borrow's only protection: `uncounted_read_sites` drives the
//     `decref_point` extension to the read's last use (region/rules.md Rule 4, the
//     borrowing node).
//   - A NATIVE read takes the Rule 5 pass-through retain, which the RC baseline honours —
//     so no lifetime extension — but ADOPTION freezes the member's RC and leaves that
//     retain inert. `counted_read_aliases` therefore drives the ownership cut's refusal
//     (adopt.md § "The lifetime obligation the root carries") and the lowerer's release
//     order at a shared point.
//
// These pins are written from that definition; the runtime witness is
// `region_container_read_borrow_uaf`.

/// The container regions recorded for the UNCOUNTED (opcode) read at `site`.
fn uncounted_containers(info: &RegionInfo, site: HirId) -> Vec<Region> {
    info.uncounted_read_sites
        .get(&site)
        .cloned()
        .unwrap_or_default()
}

/// The `(alias, container)` pairs recorded for the COUNTED (native) read at `site`.
fn counted_aliases(info: &RegionInfo, site: HirId) -> Vec<(Region, Region)> {
    info.counted_read_aliases
        .iter()
        .filter(|&&(s, _, _)| s == site)
        .map(|&(_, alias, container)| (alias, container))
        .collect()
}

#[test]
fn opcode_read_extends_container_decref_to_the_reader() {
    // `(length (%get c 0))`: the `%get` opcode borrows out of `c` without minting a
    // region of its own, and ANF leaves its result unnamed (an operand of the enclosing
    // call), so no binding chain covers it. `c`'s last MENTION is the read, but `c` is
    // still holding the value `length` derefs, so its release must land at the `length`
    // call, not at the `%get`.
    //
    // Counterfactual: with the read treated as an ordinary use, `region_data[c].
    // decref_point` IS the `%get` node — the container's free-time cascade then drops the
    // element's last count and `length` derefs a freed page (the cascade face of
    // `region_container_read_borrow_uaf`). This assertion was RED before the borrow pin.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(let [c (@array) r (string \"s\")] (begin (%array-push c r) (length (%get c 0))))",
    );
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let get_site = find_first(&hir, |h| {
        matches!(
            &h.kind,
            HirKind::Intrinsic {
                op: crate::hir::expr::IntrinsicOp::Get,
                ..
            }
        )
    })
    .expect("the shape has a %get node");
    let containers = uncounted_containers(&info, get_site);
    assert!(
        !containers.is_empty(),
        "the %get read must record its container regions (uncounted_read_sites); got none",
    );
    let c = find_binding_by_name(&hir, "c", &arena, &symbols).expect("binding c");
    let c_regions = info
        .binding_source_regions
        .get(&c)
        .expect("c holds a region");
    for r in c_regions {
        let dp = info
            .region_data
            .get(r)
            .expect("the container region has a decref_point")
            .decref_point;
        assert!(
            ord(dp) > ord(get_site),
            "the container's release (r{} at @{}) lands at or before the %get that \
             borrows out of it (@{}) — the borrowed element is freed under its reader",
            r.0,
            ord(dp),
            ord(get_site),
        );
    }
}

#[test]
fn native_read_alias_outliving_the_container_refuses_the_adopt() {
    // The NATIVE `get` borrow: `x` holds a fresh call-result placeholder region that
    // statically relates to nothing in `c`, and it is read AFTER `c`'s own last mention.
    // Under RC that is safe — the read's pass-through retain (Rule 5) hands `x` its own
    // counted reference — but adoption FREEZES the pushed element's RC, so the retain is
    // inert and `c`'s release would subtree-drop the element `x` still names. The subtree
    // must therefore stay Shared, where the retain is live again.
    //
    // Counterfactual: with only the member obligation, every member's own `decref_point`
    // sits before `c`'s release and the subtree reads admissible — the alias is invisible
    // to it, and the adopt is emitted (the subtree-drop face of
    // `region_container_read_borrow_uaf`). This assertion was RED before the alias
    // obligation.
    let (_, info, edges) = adopt_edges(
        "(let [c (@array) r (string \"s\")] \
           (begin (%array-push c r) (let [x (get c 0)] (length x))))",
    );
    assert!(
        !info.counted_read_aliases.is_empty(),
        "precondition: the shape records a native read alias; if the stdlib route \
         changed, update it",
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        adopts.is_empty(),
        "a subtree whose member can be named by a read alias that outlives the root's \
         drop must stay Shared; got adopts {:?}",
        adopts,
    );
}

#[test]
fn native_read_records_its_alias_and_container() {
    // The recorded fact the refusal and the release order both read: the native read's
    // call-result placeholder (the alias) paired with the container it reads out of.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(let [c (@array) r (string \"s\")] \
           (begin (%array-push c r) (let [x (get c 0)] (length x))))",
    );
    let get_site = *find_calls_to_primitive(&hir, "get", &arena, &symbols)
        .first()
        .expect("the shape has a `get` call");
    let pairs = counted_aliases(&info, get_site);
    assert!(
        !pairs.is_empty(),
        "the native `get` read must record its (alias, container) pairs \
         (counted_read_aliases); got none",
    );
    let alias = *info
        .alloc_region
        .get(&get_site)
        .expect("the read call mints a call-result region");
    let c = find_binding_by_name(&hir, "c", &arena, &symbols).expect("binding c");
    let c_regions = info
        .binding_source_regions
        .get(&c)
        .expect("c holds a region");
    for (a, container) in &pairs {
        assert_eq!(
            *a, alias,
            "the recorded alias must be the read call's own result region",
        );
        assert!(
            c_regions.contains(container),
            "the recorded container r{} is not one of `c`'s regions {:?}",
            container.0,
            c_regions,
        );
    }
}

#[test]
fn remove_read_is_not_a_borrow() {
    // `%pop` REMOVES its element: the funnel extracts it from the container (and from the
    // container's Owned subtree, `extract_owned_region`), handing the caller its own
    // reference. So the container is NOT borrowed from, and pinning its release to the
    // popped element's reader would be a pure over-keep. The moves-out natives are
    // excluded from the borrow face at the recording site.
    let (hir, arena, symbols, info) = analyze_with_class(
        "(let [c (@array) r (string \"s\")] \
           (begin (%array-push c r) (let [x (%pop c)] (length x))))",
    );
    let pop_sites = find_calls_to_primitive(&hir, "%pop", &arena, &symbols);
    assert!(
        !pop_sites.is_empty(),
        "precondition: the shape has a `pop` call; if the stdlib route changed, update it",
    );
    for site in pop_sites {
        assert!(
            uncounted_containers(&info, site).is_empty() && counted_aliases(&info, site).is_empty(),
            "a moves-out REMOVE (`pop`) recorded a container-read borrow at @{} — it \
             extracts the element instead, so neither the container's lifetime nor the \
             ownership cut owes it anything",
            site.0,
        );
    }
}

#[test]
fn nested_read_alias_refuses_the_adopt_transitively() {
    // Reading two levels deep: `(get (get c 0) 0)`. The INNER alias dies at the outer
    // read, so `c`'s own bound is satisfied for it — but the OUTER alias names a value
    // inside the inner member, which `c`'s drop reclaims just the same. Aliasing is
    // transitive, so the obligation closes the read edges over the member set instead of
    // stopping at the first level.
    //
    // Counterfactual: a one-level check admits this subtree — the inner alias passes and
    // the outer alias's container is the inner alias, not a member — and the outer read's
    // result is freed under its reader (the nested face of
    // `region_container_read_borrow_uaf`).
    let (_, info, edges) = adopt_edges(
        "(let [inner (@array) s (string \"s\") c (@array)] \
           (begin (%array-push inner s) (%array-push c inner) \
                  (let [x (get (get c 0) 0)] (begin (length c) (length x)))))",
    );
    assert!(
        info.counted_read_aliases.len() >= 2,
        "precondition: the shape records both read levels; got {:?}",
        info.counted_read_aliases,
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        adopts.is_empty(),
        "a subtree reachable by a CHAIN of reads whose last alias outlives the root's \
         drop must stay Shared; got adopts {:?}",
        adopts,
    );
}

#[test]
fn read_bounded_by_the_container_keeps_the_adopt() {
    // The alias obligation is a bound, not a blanket refusal: when the read's result dies
    // BEFORE the container's own release — here `c` is used again after the read — the
    // root's drop post-dominates the alias and the subtree is still adopted, so the
    // element reclaims by subtree drop. Without this pin the alias check could be written
    // as "any read refuses" and nothing would notice the reclamation it traded away.
    let (_, info, edges) = adopt_edges(
        "(let [c (@array) r (string \"s\")] \
           (begin (%array-push c r) \
                  (let [n (length (get c 0))] (%add n (length c)))))",
    );
    assert!(
        !info.counted_read_aliases.is_empty(),
        "precondition: the shape records a native read alias; if the stdlib route \
         changed, update it",
    );
    let adopts: Vec<(Region, Region)> = edges.store.values().flatten().copied().collect();
    assert!(
        !adopts.is_empty(),
        "a read whose result dies before the container's release must leave the subtree \
         adopted; got no adopt edges",
    );
}
