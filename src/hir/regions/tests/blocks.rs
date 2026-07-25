use super::*;

// ── `block`/`break`: the broken value is the block's value ────────────
//
// `break` lowers to a store into the block's result slot plus a jump to the
// block's exit label, so control leaves the body before any release the lowerer
// placed at a `decref_point` inside it. The broken value must therefore be
// anchored the way the block's own tail value is — at the `Block` node (the
// first point both the break path and the fall-through path reach) or later,
// and its regions must flow out of the block so a binding naming the block's
// value extends the release past its own uses.
// See docs/impl/region/mechanism.md § "`break` transfers its value".

/// The HirId of the first `Block` node in the tree, outermost-first.
fn first_block(hir: &Hir) -> Option<HirId> {
    if matches!(&hir.kind, HirKind::Block { .. }) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_block(c);
        }
    });
    found
}

/// The HirId of the first `String` literal node — a plain heap allocation with
/// its own region, so `alloc_region` names it directly.
fn first_string(hir: &Hir) -> Option<HirId> {
    if matches!(&hir.kind, HirKind::String(_)) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_string(c);
        }
    });
    found
}

#[test]
fn break_value_region_dies_no_earlier_than_the_block() {
    // `(block (break "s"))` — the string is the block's value, reached only by
    // the break. A `decref_point` inside the block body is jumped over, so the
    // release must be anchored at the `Block` node or later.
    let (hir, _arena, info) = pipeline("(block (break \"s\"))");
    let block = first_block(&hir).expect("a Block node");
    let s = first_string(&hir).expect("a String node");
    let r = *info
        .alloc_region
        .get(&s)
        .expect("the string literal has an alloc region");
    let dp = info
        .region_data
        .get(&r)
        .expect("the string's region has a decref_point")
        .decref_point;
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) >= ord(block),
        "broken value r{} dies at @{} (order {}), before the block @{} \
         (order {}) — the break jumps over that release",
        r.0,
        dp.0,
        ord(dp),
        block.0,
        ord(block),
    );
}

#[test]
fn break_value_region_reaches_a_binding_that_names_the_block() {
    // The block's value IS the broken value, so a binding bound to the block
    // holds the broken value's region. Without that flow the binding-chain
    // `decref_point` extension cannot see the region and the release stays at
    // the block, under the binding's later reads.
    let (hir, _arena, info) = pipeline("(let [r (block (break \"s\"))] r)");
    let s = first_string(&hir).expect("a String node");
    let string_r = *info
        .alloc_region
        .get(&s)
        .expect("the string literal has an alloc region");
    let holders: Vec<_> = info
        .binding_source_regions
        .iter()
        .filter(|(_, regions)| regions.contains(&string_r))
        .collect();
    assert!(
        !holders.is_empty(),
        "no binding names the broken value's region r{}; the block's value \
         must carry it out — binding_source_regions={:?}",
        string_r.0,
        info.binding_source_regions,
    );
}

#[test]
fn break_value_region_outlives_a_later_read_of_the_block_result() {
    // The soundness half of the flow above: `r` is read AFTER the block, so the
    // broken value's release must be anchored past that read, not at the block.
    let (hir, _arena, info) = pipeline("(let [r (block (break \"s\"))] (%string? r))");
    let block = first_block(&hir).expect("a Block node");
    let s = first_string(&hir).expect("a String node");
    let r = *info
        .alloc_region
        .get(&s)
        .expect("the string literal has an alloc region");
    let dp = info
        .region_data
        .get(&r)
        .expect("the string's region has a decref_point")
        .decref_point;
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) > ord(block),
        "broken value r{} dies at @{} (order {}) at or before the block @{} \
         (order {}) — the `(%string? r)` read would see freed pages",
        r.0,
        dp.0,
        ord(dp),
        block.0,
        ord(block),
    );
}

#[test]
fn break_through_a_branch_anchors_both_arms_at_the_block() {
    // The floor travels with the value through the tail-transparent forms: an
    // `if` in break position hands EITHER arm to the block, so neither arm may
    // be anchored inside the body.
    let (hir, _arena, info) = pipeline("(block (break (if (%int? 1) \"a\" \"b\")))");
    let block = first_block(&hir).expect("a Block node");
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let mut strings = Vec::new();
    fn collect(hir: &Hir, out: &mut Vec<HirId>) {
        if matches!(&hir.kind, HirKind::String(_)) {
            out.push(hir.id);
        }
        hir.for_each_child(|c| collect(c, out));
    }
    collect(&hir, &mut strings);
    assert_eq!(strings.len(), 2, "expected both arms' string literals");
    for s in strings {
        let r = *info.alloc_region.get(&s).expect("arm alloc region");
        let dp = info
            .region_data
            .get(&r)
            .expect("arm region decref_point")
            .decref_point;
        assert!(
            ord(dp) >= ord(block),
            "arm region r{} dies at @{} (order {}), before the block @{} \
             (order {}) — the break jumps over that release",
            r.0,
            dp.0,
            ord(dp),
            block.0,
            ord(block),
        );
    }
}
