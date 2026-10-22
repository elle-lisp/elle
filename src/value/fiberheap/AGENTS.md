# fiberheap

<!-- audited: 2026-10-20 -->

The per-VM heap: the physical region allocator
([model.md](../../../docs/impl/region/model.md)).

One `FiberHeap` per VM, shared by all of that VM's fibers; every allocation
names its heap and region explicitly through `arena`.

## Responsibility

- Own the `RegionStore`: physical region id → `RegionEntry` (pages,
  reclamation typestate, ownership children, outgoing edge table)
- Allocate `HeapObject`s, `RegionSlice` data, and the unwritten room a slice
  that grows claims ([src/lir/build/grow.rs](../../lir/build/grow.rs)), into
  specific regions
- Reclaim regions **two ways**
  ([ownership.md](../../../docs/impl/region/ownership.md)). RC (`decref` →
  cascade) is the `Counted` baseline. Subtree or set drop over an owner's
  `owned_children` serves the `Owned` forest: adoption, owner nodes, and
  cross-fiber transfer. The two modes are a mutually-exclusive `Reclaim`
  typestate, so owned-and-RC'd is unrepresentable.
- Record every cross-region reference at creation (`outgoing`), so
  reclamation walks an edge table (O(edges)) and never scans heap content
- Resolve a pointer to its region: a hydrated image through the interval
  table, every other page through the page-base walk
- Run destructors; stamp and check page identity (region id, generation, store
  id)

## Files

| File | Purpose |
|------|---------|
| [mod.rs](mod.rs) | `FiberHeap` struct, custom-allocator state, and the `needs_drop()` / `holds_value_refs()` re-export |
| [region.rs](region.rs) | The `FiberHeap` region-allocator surface: every method that allocates into, reference-counts, adopts, or inspects the per-heap `RegionStore` |
| [custom.rs](custom.rs) | The `with-allocator` stack and heap teardown: raw allocations, their destructors, and the loop `clear`/`Drop` share |
| [dropsafety.rs](dropsafety.rs) | The two exhaustive per-tag predicates region teardown drives: which variants own inner allocations needing `Drop`, and which hold `Value` references needing cascade decref |
| [regionstore.rs](regionstore.rs) | `RegionStore` + the `Reclaim` typestate (`Counted` xor `Owned`): id mint/recycle, per-id generations, the interval table, and the submodules below |
| [regionstore/alloc.rs](regionstore/alloc.rs) | Id minting, lazy entry materialization (`ensure`/`ensure_raw`), and the allocation funnel where the edge-recording cross-region incref happens once per stored object |
| [regionstore/refcount.rs](regionstore/refcount.rs) | `incref`/`decref` + cascade, `outgoing` edge recording, phantom/double-free debug asserts |
| [regionstore/ownership.rs](regionstore/ownership.rs) | The ownership forest: adoption, ownership queries, and subtree transfer, keeping the forward and back edges consistent |
| [regionstore/pointer.rs](regionstore/pointer.rs) | Pointer → region classification behind every runtime RC decision: the interval table, then the ownership-validated page-base walk |
| [regionstore/introspect.rs](regionstore/introspect.rs) | Read-only counts, byte totals, cross-ref and edge dumps behind the `arena/*` diagnostics and the free-time equivalence oracle |
| [regionstore/free.rs](regionstore/free.rs) | `free_runtime_region_pages` / `free_region_group` → the four-phase `free_region_set`: subtree / set drop over `owned_children`, frontier from the recorded `outgoing` table, and the `#[cfg(debug_assertions)]` edge-table equivalence oracle |
| [regionstore/free/rescue.rs](regionstore/free/rescue.rs) | The drop-time rescue: a member still referenced from outside the dying subtree returns to `Counted`, with its own subtree, instead of being torn down |
| [regionstore/mintscope.rs](regionstore/mintscope.rs) | closed allocation-scope mint log (macro expansion): `begin_mint_log` / `reclaim_mint_scope` RC-balance the scratch DAG by `rc − in_degree` (an `Owned` survivor is left to its owner's drop) |
| [regionstore/counters.rs](regionstore/counters.rs) | The reclamation counters behind the `arena/*` frees, sizes, adopts, rescues, extracts, reparents and owned gauges, and their process totals under `--dump=stats` |
| [regionstore/hydrate.rs](regionstore/hydrate.rs) | Install a hydrated image region: adopt the mapped pages as one interval, enter the region in the interval table, and rebuild object bookkeeping from the image's index ([image.md](../../../docs/impl/image.md)) |
| [regionpool.rs](regionpool.rs) | `RegionPool`: dual-ended pages, object and data cursors, page claim and release, and a hydrated region's interval |
| [regionpool/header.rs](regionpool/header.rs) | The 16-byte page header: region id, `(generation, store)` stamp, self-validating size tag, and the masked walk that finds a base from any pointer inside the page |
| [regionpool/introspect.rs](regionpool/introspect.rs) | `owns`, and the `find_object_cross_refs` content scan behind the cascade and the diagnostics |
| [pagepool.rs](pagepool.rs) | `PagePool`: per-thread mmap page cache by size class; the `PageDirty` spans a `--trace=scrub` release blanks; live traffic counters (`arena/page-claims`); guardfree leak hook; file-backed (hydrated image) pages bypass the cache — their release is `munmap` |
| [pagepool/page.rs](pagepool/page.rs) | `MmapPage`: a self-aligned anonymous or file-backed mapping, and the process-wide `mapped_bytes` gauge |
| [freelog.rs](freelog.rs) | `--trace=free`/`freebt` free-log; guardfree arming |
| [census.rs](census.rs) | `--trace=census` post-boot heap census: per-tag histogram, sealing classification, relocation-slot counts ([sealing.md](../../../docs/impl/image/sealing.md)) |
| [tests.rs](tests.rs) | `FiberHeap` unit tests |

## Page layout

Each region owns its pages exclusively (Rule 6). A page begins with a 16-byte
header: `region_id: u32`, `stamp: PageStamp { generation, store }`, and
`size_tag: u32` (a 24-bit `PAGE_MAGIC` in the high bits, `log2(page_size)` in
the low 8). `HeapObject` slots bump up from the header, and inline data
(`RegionSlice` payloads) bumps down from the top. Pages are self-aligned, so
`header_of_page_ptr` masks a pointer down by candidate power-of-two sizes until
a `size_tag` validates.
[generations.md](../../../docs/impl/region/generations.md) owns why the magic
and the ownership check make that walk sound.

A hydrated image region's pages tile one contiguous interval, which its pool
records. The store's interval table lists those regions, and `region_of_ptr`
and `owning_region` read it before the walk. So a pointer into an image never
reads a page header ([image.md](../../../docs/impl/image.md)).

## Region generations

[generations.md](../../../docs/impl/region/generations.md) owns the design.
`RegionStore` keeps a generation counter per physical id, bumped on every path
that returns the id's pages (`free_runtime_region_pages`, `teardown_all`).
Pages are stamped `(generation, store_id)` at claim. `region_of_ptr`, the
backend of `arena::region_of`, compares stamp to counter under
`debug_assertions` and panics on a mismatch at the deref site. The store id
scopes the comparison, so another store's page is never generation-checked.
The cascade scan resolves through the generation-blind `owning_region`.

## Invariants

1. **Destructor ordering.** `RegionPool::teardown` runs `dtors` in reverse
   allocation order before returning pages to the `PagePool`.
2. **`needs_drop` / `holds_value_refs` are exhaustive.** No wildcard arm —
   a new `HeapTag` variant is a compile error until both decide.
3. **A minted id never names a live region.** `new_runtime_region` skips
   ids with live entries (static-slot ids overlap the mint range).
4. **Generations are monotonic per store.** Bumped on every page-returning
   path, never reset; page stamps are written only at claim time.
5. **Reserved ids.** 0 = no region (unrepresentable as `RuntimeRegion`), 1 =
   reserved (never minted). Minting starts at 2, so every live region is mortal
   and RC-reclaimable.
6. **The page body belongs to the region, the header to the pool.**
   `PagePool::claim` hands out a recycled page untouched: its body holds what
   the last region wrote, and the claimant writes every slot before anything
   reads it. Only `--trace=scrub` blanks the spans a dying region wrote, and it
   leaves offset 0 alone. So a claim is a free-list pop, and a page waiting in
   the cache still carries the stamp invariant 4 depends on. See
   [model.md](../../../docs/impl/region/model.md).
7. **An interval names a live region.** A hydrated region's interval lives on
   its pool and leaves with its pages, so a recycled id starts without one. The
   store drops the region from the interval table when it releases the pages.
