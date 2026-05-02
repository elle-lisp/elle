# fiberheap

Per-fiber heap allocator with thread-local routing.

## Responsibility

- Allocate and track `HeapObject` values for a single fiber's lifetime
- Run destructors (`Drop`) for heap-allocated objects on `release()` and `clear()`
- Manage scope marks (`push_scope_mark` / `pop_scope_mark_and_release`)
- Route inter-fiber value exchange through `SharedAllocator` and outbox
- Provide thread-local install/save/restore for the active `FiberHeap`

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `FiberHeap` struct, `CustomAllocState`, `needs_drop()` |
| `routing.rs` | Thread-local `CURRENT_FIBER_HEAP`, install/save/restore helpers |
| `pool.rs` | `SlabPool` — bump arena + destructor tracking + position marks |
| `bump.rs` | `BumpArena` — byte-level sequential allocator (64KB pages) |
| `slab.rs` | `RootSlab` — legacy chunk-based typed slab (superseded by bump arena) |
| `tests.rs` | Unit tests for `FiberHeap` and routing |

## Allocator dispatch

`FiberHeap::alloc()` dispatches allocations through four layers in priority order:

1. **Outbox** (`outbox_active` true): routes to the outbox `SlabPool` for
   yield-bound values between `OutboxEnter`/`OutboxExit` bytecodes.
2. **Shared allocator** (`shared_alloc` non-null): routes all allocations to a
   `SharedAllocator` owned by the parent fiber. Used by yielding child fibers
   for the entire duration of their execution — not per-scope, per-fiber.
   (Legacy path, will be replaced by outbox once fully wired.)
3. **Custom allocator** (`custom_alloc_stack` non-empty): routes to the top
   Rust trait-object allocator; falls through to pool on null return.
4. **Private pool** (`pool: SlabPool`): bump arena with tracking. Default
   allocation path.

## SlabPool

`SlabPool` is the shared core of `FiberHeap` and `SharedAllocator`. It wraps
a `BumpArena` with allocation and destructor tracking:

- `allocs: Vec<*mut HeapObject>` — every allocation in order (for release/rotation)
- `dtors: Vec<*mut HeapObject>` — objects needing `Drop`
- `alloc_count` — running total

`mark()` captures a `SlabMark` (allocs length, dtors length, arena position).
`release(mark)` runs destructors, truncates tracking vecs, resets arena.
`teardown()` does a full reset.

`dealloc_slot()` is a no-op compatibility shim — individual slots cannot be
freed in the bump-arena model. Memory is reclaimed by scope release or teardown.

## BumpArena

`BumpArena` is a byte-level sequential allocator in `bump.rs`:

- **Pages**: `Vec<Box<[MaybeUninit<u8>]>>` — pointer-stable, heap-allocated
- **Page size**: 64KB; oversized allocations get dedicated pages
- **Allocation**: bump a byte offset within the current page
- **No individual deallocation**: reclaimed by `release_to(mark)` or `clear()`
- **Pointer stability**: pages never move once allocated

## RootSlab (legacy)

`RootSlab` in `slab.rs` is a chunk-based typed slab with intrusive free list
(256 `HeapObject` slots per chunk). It is `#[allow(dead_code)]` — superseded
by the bump arena in `SlabPool`. Retained for potential future use.

## Scope marks

`RegionEnter` pushes a mark recording the current pool position (alloc count,
dtor count, root allocs count, bump arena position). `RegionExit` pops the
mark and calls `release()` to run destructors and rewind the pool.

The lowerer gates `RegionEnter`/`RegionExit` emission on escape analysis
(`src/lir/lower/escape.rs`): only scopes where no allocated values can escape
get region instructions. The analysis checks: no captures, no suspension,
result is immediate, no outward mutation.

When a shared allocator is active (yielding child fiber), scope marks are
forwarded to the shared allocator as well, so scope-based reclamation works
correctly regardless of which pool the objects actually live in.

## Outbox

For yield-safe allocation, `FiberHeap` has an outbox mechanism:

- Parent installs an outbox (`Box<SlabPool>`) before child execution
- Child allocates between `OutboxEnter`/`OutboxExit` bytecodes into the outbox
- At yield time, values in the private pool are deep-copied to the outbox;
  values already in the outbox are returned directly
- Previous outboxes are preserved so the parent can read earlier yields
- On fiber death, all outboxes are freed in bulk

This ensures yielded values don't reference the child's private pool.

## Ownership topology

The memory model has a specific ownership structure driven by signal inference:

- **Silent fibers** allocate exclusively into their own private pool. Scope marks
  reclaim short-lived objects. `clear()` on fiber death reclaims everything.

- **Yielding child fibers** route allocations to the parent's shared allocator
  or outbox (set by `with_child_fiber`, cleared on swap-back). The child's
  private pool is essentially idle. This ensures yielded values survive the
  child's death — the parent reads them directly, zero-copy.

- **Parent fibers** own `SharedAllocator`s in `owned_shared` and outboxes in
  `old_outboxes`. `get_or_create_shared_allocator()` reuses the last allocator
  to avoid per-resume accumulation.

The chain is recursive: if a child spawns a yielding grandchild, the child
becomes a parent with its own shared allocator, and the grandchild routes
allocations there.

## Invariants

1. **Destructor ordering.** `run_dtors()` is always called before arena memory
   is reclaimed. `drop_in_place` runs on objects still in `dtors`, then the
   arena position rewinds.

2. **`allocs` and `dtors` are truncated on release.** `release(mark)` truncates
   `allocs` to `mark.allocs_len` and `dtors` to `mark.dtor_len`. The two must
   stay in sync with `alloc_count`.

3. **`needs_drop` is exhaustive.** Adding a `HeapTag` variant causes a compile
   error in `needs_drop()`. Every new variant must have an explicit `true`/`false`
   decision before the code compiles.

4. **Scope marks propagate.** `push_scope_mark` and `pop_scope_mark_and_release`
   forward to the shared allocator when `shared_alloc` is non-null. Escape
   analysis on the child's code is still the safety gate — the shared allocator
   just happens to be the backing store.
