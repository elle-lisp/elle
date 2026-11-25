// audited: 2026-11-24
//! A value a loop makes and a `break` carries out: its pass release, its exit release, and the conditions on both.
//!
//! docs/impl/region/breakloop.md
//!
//! Each subject builds the carried value with a `(string …)` call, which a
//! function runs on every pass, where it would build a literal once for the
//! whole unit (docs/impl/hoist.md). A branch tests a parameter, so the fold
//! keeps both arms.

use super::*;

/// The HirId of the first node, outermost-first, that `want` accepts.
fn first_node(hir: &Hir, want: &dyn Fn(&HirKind) -> bool) -> Option<HirId> {
    if want(&hir.kind) {
        return Some(hir.id);
    }
    let mut found = None;
    hir.for_each_child(|c| {
        if found.is_none() {
            found = first_node(c, want);
        }
    });
    found
}

/// The compiled subject, its order, and the region data of its first
/// `(string …)` call, beside the first `Block` and the first `While`, if any.
struct Subject {
    order: crate::hir::liveness::Order,
    data: RegionData,
    block: HirId,
    loop_id: Option<HirId>,
}

fn subject(source: &str) -> Subject {
    let (hir, arena, info) = pipeline(source);
    let site = find_calls_to_primitive(&hir, "string", &arena)[0];
    let r = *info
        .alloc_region
        .get(&site)
        .expect("the string call has an alloc region");
    let data = info
        .region_data
        .get(&r)
        .expect("the string's region has a decref_point")
        .clone();
    Subject {
        order: compute_order(&hir),
        data,
        block: first_node(&hir, &|k| matches!(k, HirKind::Block { .. })).expect("a Block"),
        loop_id: first_node(&hir, &|k| {
            matches!(k, HirKind::While { .. } | HirKind::Loop { .. })
        }),
    }
}

impl Subject {
    fn ord(&self, id: HirId) -> u32 {
        self.order.get(&id).copied().unwrap_or(0)
    }

    /// The loop the subject's carried value is made in.
    fn the_loop(&self) -> HirId {
        self.loop_id.expect("a While")
    }
}

/// The `forever` idiom: each pass makes `b`, and the pass that breaks carries it.
const CARRIED: &str = "(defn t [m] \
     (var k 0) \
     (forever \
       (let [b (string \"x\" k)] \
         (if (%int? m) (break b) nil)) \
       (assign k (%add k 1))))";

#[test]
fn a_carried_value_a_loop_makes_takes_a_pass_release_inside_the_loop() {
    // The counter-factual: the transfer pin moves the one release past the
    // loop, so every pass that loops on strands its string.
    let s = subject(CARRIED);
    assert!(
        s.ord(s.data.decref_point) < s.ord(s.the_loop()),
        "the pass release lands at @{} (order {}), past the loop @{} (order {}) \
         — one release cannot cover every pass",
        s.data.decref_point.0,
        s.ord(s.data.decref_point),
        s.the_loop().0,
        s.ord(s.the_loop()),
    );
}

#[test]
fn a_carried_value_a_loop_makes_takes_an_exit_release_past_the_block() {
    let s = subject(CARRIED);
    let exit = s
        .data
        .exit_point
        .expect("the carried value has an exit release");
    assert!(
        s.ord(exit) >= s.ord(s.block),
        "the exit release lands at @{} (order {}), before the block @{} (order {}) \
         — the break path would free the value before its consumer",
        exit.0,
        s.ord(exit),
        s.block.0,
        s.ord(s.block),
    );
    assert!(
        s.ord(s.data.lifetime_point) >= s.ord(exit),
        "the lifetime ends at order {}, before the exit release at order {}",
        s.ord(s.data.lifetime_point),
        s.ord(exit),
    );
}

#[test]
fn a_binding_of_the_block_value_moves_the_exit_release_not_the_pass_release() {
    // `r` names the block's value and is read after the block. That read is a
    // pin outside the loop's body, so it moves the exit release and leaves the
    // pass release inside the loop.
    let s = subject(
        "(defn t [m] \
           (var k 0) \
           (let [r (forever \
                     (let [b (string \"x\" k)] \
                       (if (%int? m) (break b) nil)) \
                     (assign k (%add k 1)))] \
             (%string? r)))",
    );
    let exit = s
        .data
        .exit_point
        .expect("the carried value has an exit release");
    assert!(
        s.ord(exit) > s.ord(s.block),
        "the exit release lands at order {}, at or before the block (order {}) \
         — `(%string? r)` would read freed pages",
        s.ord(exit),
        s.ord(s.block),
    );
    assert!(
        s.ord(s.data.decref_point) < s.ord(s.the_loop()),
        "the pass release lands at order {}, past the loop (order {})",
        s.ord(s.data.decref_point),
        s.ord(s.the_loop()),
    );
}

#[test]
fn a_carried_value_made_outside_a_loop_takes_one_release() {
    // No loop makes the value, so one release covers it: the transfer pin's.
    let s = subject(
        "(defn t [u] \
           (block (let [b (string \"x\" u)] (if (%int? u) (break b) nil) 0)))",
    );
    assert_eq!(
        s.data.exit_point, None,
        "a value made once needs no second release"
    );
}

#[test]
fn a_carried_value_no_binder_holds_takes_one_release() {
    // The string is made in the break's own value, so no binder's slot names
    // it and no value route can release it twice. It keeps the transfer pin's
    // one release, and a pass that loops on makes no string to strand.
    let s = subject(
        "(defn t [m] \
           (var k 0) \
           (forever \
             (if (%int? m) (break (string \"x\" k)) nil) \
             (assign k (%add k 1))))",
    );
    assert_eq!(
        s.data.exit_point, None,
        "a value with no value route keeps the one release"
    );
}

#[test]
fn a_value_a_mutated_binder_holds_takes_one_release() {
    // `b` is reassigned, so its slot is a 1-slot container whose store drops
    // the displaced value. A value release through it would free whatever the
    // slot holds then, so the region keeps its one release.
    let s = subject(
        "(defn t [m] \
           (var k 0) \
           (var b nil) \
           (forever \
             (assign b (string \"x\" k)) \
             (if (%int? m) (break b) nil) \
             (assign k (%add k 1))))",
    );
    assert_eq!(
        s.data.exit_point, None,
        "a mutated binder's value keeps the one release"
    );
}
