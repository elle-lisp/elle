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

// ── the regions the break jumps OVER ─────────────────────────────────
//
// The same jump that strands the broken value strands every other release the
// lowerer placed between the break site and the exit label — those regions have
// no consumer to be handed to, so they simply never die. They are re-anchored to
// the same point the broken value takes, `last_use[block]`, which both paths
// reach. Three boundaries stop the hoist: a nested loop (the value is
// re-allocated per iteration, so one release cannot cover N), a nested lambda
// (its releases run in another activation, against another frame's slots), and a
// frame-replacing exit in the body (which leaves the anchor unreached on the
// fall-through path).
// See docs/impl/region/mechanism.md § "A release the break jumps over is not a
// release".

/// The HirId of the first `While`/`Loop` node, outermost-first.
fn first_loop(hir: &Hir) -> Option<HirId> {
    if matches!(&hir.kind, HirKind::While { .. } | HirKind::Loop { .. }) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_loop(c);
        }
    });
    found
}

/// The `decref_point` of the region allocated at the first `String` literal.
fn first_string_decref_point(hir: &Hir, info: &RegionInfo) -> HirId {
    let s = first_string(hir).expect("a String node");
    let r = *info
        .alloc_region
        .get(&s)
        .expect("the string literal has an alloc region");
    info.region_data
        .get(&r)
        .expect("the string's region has a decref_point")
        .decref_point
}

#[test]
fn a_release_the_break_jumps_over_is_pinned_to_the_block() {
    // `x` is NOT the broken value — the break carries `1` — but its release sits
    // after the break site inside the body, so the jump to the exit label passes
    // over it and it never runs at all.
    let (hir, _arena, info) =
        pipeline("(block (let [x \"s\"] (if (%int? 1) (break 1) nil) (%string? x)))");
    let block = first_block(&hir).expect("a Block node");
    let dp = first_string_decref_point(&hir, &info);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) >= ord(block),
        "skipped region dies at @{} (order {}), inside the block @{} \
         (order {}) — the break jumps over that release",
        dp.0,
        ord(dp),
        block.0,
        ord(block),
    );
}

#[test]
fn a_release_before_the_break_site_stays_where_it_is() {
    // The window opens at the break site: a release the break path has ALREADY
    // run must not be deferred to the exit label. Promptness, not soundness —
    // but a pass that hoists the whole body has stopped being a window.
    let (hir, _arena, info) =
        pipeline("(block (let [x \"s\"] (%string? x)) (if (%int? 1) (break 1) nil) 0)");
    let block = first_block(&hir).expect("a Block node");
    let dp = first_string_decref_point(&hir, &info);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) < ord(block),
        "region used entirely before the break dies at @{} (order {}), at or \
         after the block @{} (order {}) — its release already ran on both paths",
        dp.0,
        ord(dp),
        block.0,
        ord(block),
    );
}

#[test]
fn a_block_with_no_break_leaves_its_releases_in_the_body() {
    // The control for the pin above: with no break there is no jump and no
    // window, so nothing moves.
    let (hir, _arena, info) = pipeline("(block (let [x \"s\"] (%string? x)) 0)");
    let block = first_block(&hir).expect("a Block node");
    let dp = first_string_decref_point(&hir, &info);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) < ord(block),
        "region in a break-free block dies at @{} (order {}), at or after the \
         block @{} (order {}) — nothing jumps over its release",
        dp.0,
        ord(dp),
        block.0,
        ord(block),
    );
}

#[test]
fn a_loop_nested_in_the_block_keeps_its_per_iteration_release() {
    // The value is re-allocated per iteration, so hoisting its release to the
    // block's exit label would leave ONE release for N allocations — a worse
    // leak than the skip. The window stops at the loop.
    let (hir, _arena, info) = pipeline(
        "(block (if (%int? 1) (break 1) nil) \
           (while (%int? 0) (let [x \"s\"] (%string? x))) 0)",
    );
    let loop_id = first_loop(&hir).expect("a While node");
    let dp = first_string_decref_point(&hir, &info);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) <= ord(loop_id),
        "loop-body region dies at @{} (order {}), past the loop @{} \
         (order {}) — one release cannot cover a per-iteration allocation",
        dp.0,
        ord(dp),
        loop_id.0,
        ord(loop_id),
    );
}

#[test]
fn a_lambda_nested_in_the_block_keeps_its_own_activations_release() {
    // The lambda body's releases run in a different activation against a
    // different frame's slots; the enclosing block's exit label is not a point
    // that activation reaches.
    let (hir, _arena, info) = pipeline(
        "(block (if (%int? 1) (break 1) nil) \
           (let [f (fn [] (let [x \"s\"] (%string? x)))] (f)))",
    );
    let block = first_block(&hir).expect("a Block node");
    let dp = first_string_decref_point(&hir, &info);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) < ord(block),
        "lambda-body region dies at @{} (order {}), at or after the enclosing \
         block @{} (order {}) — that release belongs to the closure's frame",
        dp.0,
        ord(dp),
        block.0,
        ord(block),
    );
}

#[test]
fn a_frame_replacing_exit_in_the_body_refuses_the_window() {
    // The hoist's premise is that the block's exit label is a point EVERY path
    // reaches. A tail call in the body replaces the frame, so the fall-through
    // path leaves through the callee instead of arriving at the exit label — a
    // release moved there would be dead on exactly the path that used to run it.
    let (hir, _arena, info) = pipeline(
        "(defn g [n] n) \
         (defn h [n] (block (if (%int? 1) (break 1) nil) \
                        (let [x \"s\"] (%string? x)) \
                        (g n)))",
    );
    let block = first_block(&hir).expect("a Block node");
    let dp = first_string_decref_point(&hir, &info);
    let order = compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    assert!(
        ord(dp) < ord(block),
        "region in a body that tail-calls dies at @{} (order {}), at or after \
         the block @{} (order {}) — the frame is gone before the exit label",
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
