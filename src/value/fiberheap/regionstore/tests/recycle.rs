use super::*;

// ── Physical id recycling (docs/impl/region/model.md § "Physical id recycling") ─
//
// Written from the spec: every physical id returns to `free_physical`, whether a
// teardown freed it or a mint never materialized it.

#[test]
fn an_unmaterialized_mint_returns_its_id_to_the_free_list() {
    // The reserved → free exit. A minted id that nothing allocates into names no
    // region at all: no entry, no pages, no count. Recycling it must make the
    // next mint hand it straight back.
    //
    // Counter-factual: without the recycle the second mint takes a fresh id off
    // `next_physical`, and every such mint raises the largest id forever.
    let mut store = RegionStore::default();
    let mint = store.new_runtime_region_tracked();

    store.recycle_unmaterialized(mint);

    assert_eq!(
        store.new_runtime_region(),
        mint.region(),
        "an id no allocation materialized must be reissued, not stranded",
    );
}

#[test]
fn recycle_leaves_a_materialized_id_alone() {
    // A mint that DID allocate holds a live region, and the recycle must pass
    // over it: the id is not reissued and the region keeps its count.
    let mut store = RegionStore::default();
    let mint = store.new_runtime_region_tracked();
    store.alloc_obj(mint.region(), cons_obj());

    store.recycle_unmaterialized(mint);

    assert_ne!(
        store.new_runtime_region(),
        mint.region(),
        "a live region's id must not be reissued",
    );
    assert_eq!(
        store.rc(mint.region()),
        1,
        "the live region is untouched by the recycle",
    );
}

#[test]
fn recycling_a_live_id_does_not_double_book_it_for_its_later_free() {
    // Why the liveness check is not redundant with the mint loop's skip. Push a
    // still-live id and the immediate damage is hidden: the next mint pops it,
    // sees it live, and skips it. The damage lands later — when the region is
    // genuinely freed, its teardown pushes the same id a SECOND time, and now
    // neither copy is live. Two mints take it, and the first to allocate has its
    // pages freed under the second.
    //
    // Counter-factual: with the liveness check removed, the two mints below
    // return the same id. The `assert_ne!` in
    // `recycle_leaves_a_materialized_id_alone` does not catch that on its own —
    // the skip loop masks it until the free.
    let mut store = RegionStore::default();
    let mint = store.new_runtime_region_tracked();
    let r = mint.region();
    store.alloc_obj(r, cons_obj());

    store.recycle_unmaterialized(mint); // live: must push nothing
    store.decref(r); // freed: the teardown pushes the id, once

    let first = store.new_runtime_region();
    let second = store.new_runtime_region();
    assert_eq!(first, r, "the teardown's recycle reissues the id");
    assert_ne!(
        second, first,
        "the id reached the free list twice — two logical regions on one id",
    );
}

#[test]
fn recycle_refuses_an_id_freed_since_the_mint() {
    // The trap the generation check exists for. A region that materialized and
    // was freed between the mint and the recycle ALSO leaves `regions[id]`
    // empty — but its teardown already pushed that id, so pushing it again puts
    // a duplicate in `free_physical`.
    //
    // A duplicate is not caught downstream: `new_runtime_region` skips an id
    // that is already LIVE, and neither of two mints taking the same
    // still-unmaterialized id is live yet. Both would get the id, and the first
    // to allocate would have its pages freed under the second.
    //
    // Counter-factual: an emptiness-only check (no generation comparison) makes
    // the two mints below return the same id.
    let mut store = RegionStore::default();
    let mint = store.new_runtime_region_tracked();
    let r = mint.region();
    store.alloc_obj(r, cons_obj());
    store.decref(r); // freed: pages returned, generation bumped, id recycled once

    store.recycle_unmaterialized(mint);

    let first = store.new_runtime_region();
    let second = store.new_runtime_region();
    assert_eq!(first, r, "the teardown's own recycle still reissues the id");
    assert_ne!(
        second, first,
        "the id must appear in the free list once, not twice",
    );
}

#[test]
fn recycle_refuses_an_id_that_lived_and_died_twice_since_the_mint() {
    // The same trap one incarnation deeper, where comparing only "has the
    // generation moved by exactly one" could still be fooled: the id is freed,
    // reminted, and freed again. Its generation is two past the mint and it sits
    // in the free list once. The recycle must still refuse it.
    let mut store = RegionStore::default();
    let mint = store.new_runtime_region_tracked();
    let r = mint.region();
    store.alloc_obj(r, cons_obj());
    store.decref(r);
    let again = store.new_runtime_region();
    assert_eq!(again, r, "the freed id is reissued");
    store.alloc_obj(again, cons_obj());
    store.decref(again);

    store.recycle_unmaterialized(mint);

    let first = store.new_runtime_region();
    let second = store.new_runtime_region();
    assert_eq!(first, r);
    assert_ne!(second, first, "no duplicate reached the free list");
}

#[test]
fn a_run_of_unmaterialized_mints_does_not_grow_the_region_table() {
    // The resident-memory claim itself, at the store level. The table is
    // `Vec<Option<RegionEntry>>` indexed by id, so its length is one past the
    // largest id ever made live. Mints that allocate nothing must not move it.
    //
    // Counter-factual: unrecycled, each iteration strands one id, and the table
    // jumps to the iteration count the moment any later mint materializes.
    let mut store = RegionStore::default();
    // A live region, so the table has a length to hold steady.
    let keep = store.new_runtime_region();
    store.alloc_obj(keep, cons_obj());
    let before = store.region_table_len();

    for _ in 0..10_000 {
        let mint = store.new_runtime_region_tracked();
        store.recycle_unmaterialized(mint);
    }
    // Materialize one region afterwards: the table grows only when an id is made
    // live, so a stranded id shows up here rather than inside the loop.
    let after_loop = store.new_runtime_region();
    store.alloc_obj(after_loop, cons_obj());

    let len = store.region_table_len();
    assert!(
        len <= before + 1,
        "10000 mints that allocated nothing grew the region table from {before} \
         to {len} entries — their ids never returned to the free list",
    );
}

#[test]
fn a_recycled_id_mints_a_sound_region_afterwards() {
    // The recycled id is an ordinary free id, not a special one: the next mint
    // takes it, allocates into it, and the value resolves back to that region
    // through the page header. A recycle that left stale generation or table
    // state behind would trip the stale-deref check here instead.
    let mut store = RegionStore::default();
    let mint = store.new_runtime_region_tracked();
    store.recycle_unmaterialized(mint);

    let r = store.new_runtime_region();
    assert_eq!(r, mint.region());
    let v = store.alloc_obj(r, cons_obj());
    assert_eq!(store.region_of_ptr(v.as_heap_ptr().unwrap()), r.get());
    assert_eq!(store.rc(r), 1);

    store.decref(r);
    assert_eq!(
        store.rc(r),
        0,
        "the region frees normally after the recycle"
    );
}
