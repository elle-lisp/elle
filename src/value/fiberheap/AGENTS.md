# fiberheap

The per-VM heap: the physical region allocator (docs/impl/region/model.md). One
`FiberHeap` per VM, shared by all of that VM's fibers; every allocation names its
heap and region explicitly through `arena`.

## Responsibility

- Own the `RegionStore`: physical region id → `RegionEntry` (pages + reclamation
  typestate + ownership children + outgoing edge table)
- Allocate `HeapObject`s and `RegionSlice` data into specific regions
- Reclaim regions **two ways** (docs/impl/region/ownership.md § "Adoption and subtree drop"):
  by **RC** (`decref` → cascade — the `Counted` baseline) and by **subtree / set drop**
  over an owner's `owned_children` (the `Owned` forest — adoption, owner nodes, and
  cross-fiber transfer). The two modes are a mutually-exclusive `Reclaim` typestate, so
  owned-and-RC'd is unrepresentable.
- Record every cross-region reference at creation (`outgoing`) so reclamation walks an
  edge table (O(edges)), never a heap-content scan
- Run destructors; stamp and check page identity (region id, generation, store id)

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `FiberHeap` struct, custom-allocator stack, `needs_drop()`, `holds_value_refs()` |
| `regionstore.rs` | `RegionStore` + the `Reclaim` typestate (`Counted` xor `Owned`): id mint/recycle, per-id generations, `region_of_ptr`, and the ownership-forest primitives `adopt_region` / `reparent_owned_children` / `region_is_owned` (`owned_children` + `outgoing` on each `RegionEntry`) |
| `regionstore/refcount.rs` | `incref`/`decref` + cascade, `outgoing` edge recording, phantom/double-free debug asserts |
| `regionstore/free.rs` | `free_runtime_region_pages` / `free_region_group` → the four-phase `free_region_set`: subtree / set drop over `owned_children`, frontier from the recorded `outgoing` table, and the `#[cfg(debug_assertions)]` edge-table equivalence oracle |
| `regionstore/mintscope.rs` | closed allocation-scope mint log (macro expansion): `begin_mint_log` / `reclaim_mint_scope` RC-balance the scratch DAG by `rc − in_degree` (an `Owned` survivor is left to its owner's drop) |
| `regionpool.rs` | `RegionPool`: dual-ended pages, page-header stamp, `header_of_page_ptr` |
| `regionpool/introspect.rs` | `find_object_cross_refs` content scan (cascade + diagnostics) |
| `pagepool.rs` | `PagePool`: per-thread mmap page cache by size class; guardfree leak hook |
| `freelog.rs` | `--trace=free`/`freebt` free-log; guardfree arming |
| `tests.rs` | `FiberHeap` unit tests |

## Page layout

Each region owns its pages exclusively (Rule 6). A page begins with a
16-byte header: `region_id: u32`, `stamp: PageStamp { generation, store }`,
`size_tag: u32` (a 24-bit `PAGE_MAGIC` in the high bits, `log2(page_size)` in
the low 8). `HeapObject` slots bump up from the header; inline data
(`RegionSlice` payloads) bumps down from the top. Pages are self-aligned, so
`header_of_page_ptr` masks a pointer down by candidate power-of-two sizes
until the `size_tag` magic + log2 matches — O(1) region attribution. The
magic matters: a smaller sub-alignment of a *large* page lands mid-page on
object data, where a bare `log2` byte could coincidentally match a smaller
size and be read as a garbage header (the `oracle.lisp` 584 GB `ensure_raw`
blowup). The magic makes that ~`1/2^32`; the authoritative resolver,
`RegionStore::region_of_ptr`, additionally requires the matched region to
*own* the pointer, so a mid-page coincidence never wins over the true base.

## Region generations (docs/impl/region/generations.md § "Region generations")

`RegionStore` keeps a generation counter per physical id, bumped on every
path that returns the id's pages (`free_runtime_region_pages`,
`teardown_all`); a recycled id mints its next region at the bumped value.
Pages are stamped `(generation, store_id)` at claim. `region_of_ptr` — the
backend of `arena::region_of`, the funnel every runtime RC decision flows
through — compares stamp to counter under `debug_assertions` and panics on
mismatch: a stale deref detonates at the deref site instead of reading a
freed-but-cached page. The store id (process-unique per `RegionStore`)
scopes the comparison; another store's page is never generation-checked
(generations across stores are unrelated). The cascade scan deliberately
bypasses the check via the generation-blind `region_of_page_ptr`.

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
