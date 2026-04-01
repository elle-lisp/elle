# fiberheap

Per-fiber heap allocator with thread-local routing.

## Responsibility

- Allocate and track `HeapObject` values for a single fiber's lifetime
- Run destructors (`Drop`) for heap-allocated objects on `release()` and `clear()`
- Manage scope marks (`push_scope_mark` / `pop_scope_mark_and_release`)
- Route inter-fiber value exchange through `SharedAllocator`
- Provide thread-local install/save/restore for the active `FiberHeap`

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `FiberHeap` struct, `CustomAllocState`, `needs_drop()` |
| `routing.rs` | Thread-local `CURRENT_FIBER_HEAP`, install/save/restore helpers |
| `slab.rs` | `RootSlab` — chunk-based typed slab with intrusive free list |
| `tests.rs` | Unit tests for `FiberHeap` and routing |

## Allocator dispatch

`FiberHeap::alloc()` dispatches allocations through three layers in priority order:

1. **Shared allocator** (`shared_alloc` non-null): routes all allocations to a
   `SharedAllocator` owned by the parent fiber. Used by yielding child fibers
   for the entire duration of their execution — not per-scope, per-fiber.
2. **Custom allocator** (`custom_alloc_stack` non-empty): routes to the top
   Rust trait-object allocator; falls through to slab on null return.
3. **Root slab** (`root_slab: RootSlab`): chunk-based typed slab with
   intrusive free list. Default allocation path.

## Root slab (`RootSlab`)

`root_slab` is a chunk-based typed slab allocator (`slab.rs`). Each chunk is a
`Box<[MaybeUninit<HeapObject>]>` with 256 slots — pointer-stable, heap-allocated.

Key properties:
- `alloc()` reuses a free-list slot or bumps a cursor within the last chunk
- `dealloc(ptr)` returns a slot to the intrusive free list (reused on next alloc)
- `allocated_bytes()` reflects committed chunk memory (not live objects)
- `clear()` keeps the first chunk, drops the rest, resets free list and cursor

`root_allocs: Vec<*mut HeapObject>` tracks all root-slab allocations in order.
`release(mark)` uses `mark.root_allocs_len()` to dealloc only the post-mark slots.
This makes `release()` return slab memory to the free list, bounding memory at the
working-set size rather than growing monotonically.

## Scope marks

`RegionEnter` pushes a mark recording the current slab position (alloc count,
dtor count, root allocs count). `RegionExit` pops the mark and calls `release()`
to run destructors and return slab slots to the free list for objects allocated
within the scope.

The lowerer gates `RegionEnter`/`RegionExit` emission on escape analysis
(`src/lir/lower/escape.rs`): only scopes where no allocated values can escape
get region instructions. The analysis checks: no captures, no suspension,
result is immediate, no outward mutation.

When a shared allocator is active (yielding child fiber), scope marks are
forwarded to the shared allocator as well, so scope-based reclamation works
correctly regardless of which slab the objects actually live in.

## Ownership topology

The memory model has a specific ownership structure driven by signal inference:

- **Silent fibers** allocate exclusively into their own `root_slab`. Scope marks
  reclaim short-lived objects. `clear()` on fiber death reclaims everything.

- **Yielding child fibers** route all allocations to their parent's
  `SharedAllocator` (set by `with_child_fiber`, cleared on swap-back). The
  child's private slab is essentially idle. This ensures yielded values survive
  the child's death — the parent reads them directly, zero-copy.

- **Parent fibers** own `SharedAllocator`s in `owned_shared`. The shared
  allocator is also a `RootSlab` with its own destructor and scope-mark
  tracking. `get_or_create_shared_allocator()` reuses the last allocator to
  avoid per-resume accumulation.

The chain is recursive: if a child spawns a yielding grandchild, the child
becomes a parent with its own shared allocator, and the grandchild routes
allocations there.

## Invariants

1. **Destructor ordering.** `run_dtors()` is always called before slab memory
   is reclaimed. `drop_in_place` runs on objects still in `dtors`, then the slab
   slot is freed.

2. **`root_allocs` mirrors `root_slab.live_count`.** Every root-slab `alloc()`
   appends to `root_allocs`; every `release()` or `clear()` pops the tail and
   calls `root_slab.dealloc()`. The two must stay in sync.

3. **`needs_drop` is exhaustive.** Adding a `HeapTag` variant causes a compile
   error in `needs_drop()`. Every new variant must have an explicit `true`/`false`
   decision before the code compiles.

4. **Scope marks propagate.** `push_scope_mark` and `pop_scope_mark_and_release`
   forward to the shared allocator when `shared_alloc` is non-null. Escape
   analysis on the child's code is still the safety gate — the shared allocator
   just happens to be the backing store.
