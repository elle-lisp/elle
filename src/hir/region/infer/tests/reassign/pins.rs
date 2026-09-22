// audited: 2026-09-22
//! Where a stored value's producer release is pinned, and where the cell's own
//! content drop lands.
//!
//! docs/impl/region/bindings.md

use super::*;

/// An aliased STORED value takes the model (docs/impl/region/bindings.md § "The
/// store-site pin asks only that the store run once per binding of the name it
/// reads"). `v` is a second name for the value each iteration stores, and it
/// refuses nothing: the pin rule is a maximum and `v`'s own reads extend the
/// stored region's release through the binding chain, so the release lands at or
/// after `(%length v)`. What refusing bought instead was the baseline, whose one
/// chain-extended release rode the cell's uses past the loop and served a region
/// minted per iteration — the conditional-accumulate strand behind
/// elle-lisp/elle#1186.
///
/// The stored region rides the phi onto the binding's own source set, so the
/// aliased overlap withholds the donation and the cell counts its init at the
/// chain source's binder instead; nothing is suppressed.
#[test]
fn reassign_gate_counts_an_aliased_assign_value() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (let [v (array i 7)]\n\
                      (assign last v)\n\
                      (%length v)\n\
                      (assign i (%add i 1))))\n\
                  0)))\n\
         (h 3)",
    );
    let (last, last_sites) = heap_carrying_reassign(&hir, &info);
    assert!(
        info.cell_containers.contains_key(&last),
        "an aliased stored value must take the container model — the store-site \
         pin is what keeps a loop's releases inside the loop"
    );
    assert!(
        last_sites
            .iter()
            .any(|site| info.drop_on_overwrite_sites.contains(site)),
        "the counted store's drop-on-overwrite releases the cell's OWN reference"
    );
    for r in &info.binding_source_regions[&last] {
        assert!(
            !info.suppressed_decref_regions.contains(r),
            "an aliased overlap suppresses nothing — the alias keeps the decref \
             that releases the producer's reference (region {r:?})",
        );
    }
    assert!(
        !info.counted_cell_init_sites.is_empty(),
        "the aliased overlap withholds the donation, so the cell counts its \
         init at the chain source's binder",
    );
}

/// The content drop POST-DOMINATES every store (docs/impl/region/bindings.md
/// § "Where the content drop lands"). A store inside a loop inside one branch
/// arm seeds the demise there — a point no other arm's path reaches, and one a
/// later iteration re-enters — so the drop must hoist to the nodes of the loop
/// and the branch, where the lowerer emits it after each on every path.
///
/// The counter-factual: left at the seed, the drop frees the just-stored value
/// once per iteration on the storing path and never on any other, and nothing
/// fails on it because each path alone stays consistent with SOME accounting.
/// This is the `each`-expansion face of elle-lisp/elle#1186: the macro's arms
/// each store the loop's element into the outer binding, and the seed lands in
/// whichever arm is structurally last.
#[test]
fn cell_content_drop_postdominates_arm_stores() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (t)\n\
           (let [@u nil]\n\
             (var i 0)\n\
             (if t\n\
                 (while (%lt i 2)\n\
                   (begin (let [x (%pair i i)] (assign u x))\n\
                          (assign i (%add i 1))))\n\
                 nil)\n\
             1)))\n\
         (h 1)",
    );
    let order = crate::hir::liveness::compute_order(&hir);
    let ord = |id: HirId| order.get(&id).copied().unwrap_or(0);
    let (u, sites) = heap_carrying_reassign(&hir, &info);
    let cell = info
        .cell_containers
        .get(&u)
        .expect("the in-arm store takes the container model");
    for site in &sites {
        assert!(
            ord(cell.demise) > ord(*site),
            "the content drop at @{} must post-date the store at @{} — a drop \
             seeded inside the storing arm's loop runs per iteration there and \
             never on the arm that stores nothing",
            cell.demise.0,
            site.0
        );
    }
}

/// A value stored into a 1-slot container is released at its STORE site, and a
/// reader the cell's own reference already outlives may not drag that release
/// forward — neither a cell binding's uses nor an uncounted opcode read of the
/// cell (`%get`/`%first`/`%rest`), whose borrow the cell protects. Both routes
/// reach past the loop that stores a fresh value every iteration, so one release
/// would cover N allocations (docs/impl/region/bindings.md § "A chain of
/// forwarding edges hands one reference along, so the fold follows it whole").
#[test]
fn a_cell_stored_value_is_not_extended_by_a_read_of_the_cell() {
    // The read sits in statement position: an uncounted read in TAIL position
    // returns a borrow out of the container, which transfers the cell's
    // reference and refuses the model outright.
    let (hir, info) = two_loop_chain("", "(begin (%get last 1) 0)");
    let links = chain_links(&hir, &info);
    assert_eq!(links.len(), 2, "precondition: the chain has two links");
    let read_regions: Vec<Region> = info
        .uncounted_read_sites
        .values()
        .flatten()
        .copied()
        .collect();
    assert!(
        !read_regions.is_empty(),
        "precondition: the `%get` records an uncounted read of the cell"
    );
    for b in &links {
        let cell = info
            .cell_containers
            .get(b)
            .unwrap_or_else(|| panic!("precondition: {b:?} takes the container model"));
        let store = cell
            .stores
            .sites()
            .last()
            .expect("the link stores at least once");
        for r in &cell.stores.value_region_set() {
            assert!(
                read_regions.contains(r),
                "precondition: the read names the cell's stored value region {r:?}"
            );
            assert_eq!(
                info.region_data[r].decref_point, store,
                "a stored value's release stays at its store site: the reader \
                 borrows through the cell's own reference, and one release at \
                 the read cannot cover a loop's worth of stores",
            );
        }
    }
}

/// A stored value's producer release is pinned to the store that took **that**
/// value, not to the cell's latest store. Two `assign`s in mutually exclusive
/// arms of a branch inside a loop are the ordinary shape: reading the cell's
/// stores as one set pins the first arm's value inside the SECOND arm, so an
/// iteration taking the first arm again displaces the previous value from its own
/// ANF slot before that pin ever runs — one stranded region per repeat, growing
/// with the iteration count (docs/impl/region/bindings.md § "The store site is
/// the store that took THAT value"; `tests/elle/region-cell-arm-store.lisp`).
///
/// Stated over the program rather than over the container's fields: each stored
/// value's release must land inside the subtree of the `assign` that stored it.
#[test]
fn a_stored_value_is_pinned_to_the_store_that_took_it() {
    let (hir, _, info) = pipeline(
        "(def @h (fn (n)\n\
           (begin (var last (array 0 0))\n\
                  (var i 0)\n\
                  (while (%lt i n)\n\
                    (begin (if (%lt i 2)\n\
                             (assign last (array i 7))\n\
                             (assign last (array i 9)))\n\
                           (assign i (%add i 1))))\n\
                  0)))\n\
         (h 8)",
    );
    // The heap-carrying cell of the loop, and the two arms that store into it.
    let (cell_binding, stores) = info
        .cell_containers
        .iter()
        .map(|(b, c)| (*b, c.stores.len()))
        .find(|&(_, n)| n > 1)
        .expect("precondition: one cell is stored into from both arms");
    assert_eq!(
        stores, 2,
        "precondition: the branch gives the cell exactly two store sites"
    );
    let sites: Vec<HirId> = find_reassign_sites(&hir)
        .into_iter()
        .filter(|&(_, b)| b == cell_binding)
        .map(|(id, _)| id)
        .collect();
    assert_eq!(sites.len(), 2, "precondition: two assigns name the cell");

    // Each arm allocates the value it stores, so the region born under one
    // `assign` must be released under that same `assign`.
    for site in sites {
        let born: Vec<Region> = info
            .alloc_region
            .iter()
            .filter(|(alloc_id, _)| subtree_contains(&hir, site, **alloc_id))
            .map(|(_, &r)| r)
            .collect();
        assert!(
            !born.is_empty(),
            "precondition: the assign at @{} allocates the value it stores",
            site.0
        );
        for r in born {
            let Some(d) = info.region_data.get(&r) else {
                continue;
            };
            assert!(
                subtree_contains(&hir, site, d.decref_point),
                "a value allocated under the assign at @{} is released at @{}, \
                 outside that assign: the pin followed the cell's LAST store \
                 instead of the store that took this value, so every path that \
                 does not reach the last store strands it",
                site.0,
                d.decref_point.0,
            );
        }
    }
}

/// True when `id` is `root` or lies in `root`'s subtree.
fn subtree_contains(hir: &Hir, root: HirId, id: HirId) -> bool {
    fn walk(hir: &Hir, root: HirId, id: HirId, inside: bool) -> bool {
        let inside = inside || hir.id == root;
        if inside && hir.id == id {
            return true;
        }
        let mut found = false;
        hir.for_each_child(|c| found |= walk(c, root, id, inside));
        found
    }
    walk(hir, root, id, false)
}
