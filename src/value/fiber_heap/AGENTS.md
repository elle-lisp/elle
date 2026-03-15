# fiber_heap

Per-fiber heap allocator with thread-local routing.

## Responsibility

- Allocate and track `HeapObject` values for a single fiber's lifetime
- Run destructors (`Drop`) for heap-allocated objects on `release()` and `clear()`
- Manage scope-region bump allocators (`push_scope_mark` / `pop_scope_mark_and_release`)
- Route inter-fiber value exchange through `SharedAllocator`
- Provide thread-local install/save/restore for the active `FiberHeap`

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | `FiberHeap` struct, `ActiveAlloc` enum, `needs_drop()` |
| `routing.rs` | Thread-local `CURRENT_FIBER_HEAP`, install/save/restore helpers |
| `slab.rs` | `RootSlab` — chunk-based typed slab with intrusive free list |
| `tests.rs` | Unit tests for `FiberHeap` and routing |

## Allocator dispatch

`FiberHeap::alloc()` dispatches allocations through three layers in priority order:

1. **Shared allocator** (`shared_alloc` non-null): routes all allocations to a
   `SharedAllocator` owned by the parent fiber. Used by yielding child fibers.
2. **Custom allocator** (`custom_alloc_stack` non-empty): routes to the top
   Rust trait-object allocator; falls through to slab on null return.
3. **Active allocator** (`active_allocator: ActiveAlloc`):
   - `Slab` — allocates from `root_slab` and appends pointer to `root_allocs`
   - `Bump(ptr)` — allocates from the top scope bump (inside a `RegionEnter`)

## Root slab (`RootSlab`)

`root_slab` is a chunk-based typed slab allocator (`slab.rs`). Each chunk is a
`Box<[MaybeUninit<HeapObject>]>` — pointer-stable, heap-allocated.

Key properties:
- `dealloc(ptr)` returns a slot to the intrusive free list (reused on next alloc)
- `allocated_bytes()` reflects committed chunk memory (not live objects)
- `clear()` keeps the first chunk, drops the rest, resets free list and cursor

`root_allocs: Vec<*mut HeapObject>` tracks all root-slab allocations in order.
`release(mark)` uses `mark.root_allocs_len()` to dealloc only the post-mark slots.
This makes `release()` return slab memory to the free list, bounding memory at the
working-set size rather than growing monotonically.

## Scope bumps

Each `push_scope_mark()` pushes a fresh `bumpalo::Bump` onto `scope_bumps` and
sets `active_allocator = ActiveAlloc::Bump(ptr)`. All allocations within the scope
go to this bump; no per-object dealloc is needed — `pop_scope_mark_and_release()`
drops the entire bump atomically after running destructors.

## Active allocator (`ActiveAlloc`)

```rust
pub(crate) enum ActiveAlloc {
    Slab,
    Bump(*const bumpalo::Bump),
}
```

- Starts as `ActiveAlloc::Slab` at construction.
- `push_scope_mark()` sets it to `Bump(ptr)`.
- `pop_scope_mark_and_release()` restores it to `Bump(prev)` or `Slab`.
- Saved/restored by `execute_bytecode_saving_stack` (call/return).
- Each fiber owns its own `FiberHeap`, so fiber swap is implicit.

`save_active_allocator()` / `restore_active_allocator()` operate on the
thread-local `CURRENT_FIBER_HEAP`. Both are `pub(crate)`.

## Invariants

1. **Destructor ordering.** `run_dtors()` is always called before slab/bump memory
   is reclaimed. `drop_in_place` runs on objects still in `dtors`, then the slab
   slot or bump chunk is freed.

2. **`root_allocs` mirrors `root_slab.live_count`.** Every root-slab `alloc()`
   appends to `root_allocs`; every `release()` or `clear()` pops the tail and
   calls `root_slab.dealloc()`. The two must stay in sync.

3. **Scope allocations never appear in `root_allocs`.** `ActiveAlloc::Bump` paths
   skip the `root_allocs.push()`. Scope memory is reclaimed by dropping the bump.

4. **`needs_drop` is exhaustive.** Adding a `HeapTag` variant causes a compile
   error in `needs_drop()`. Every new variant must have an explicit `true`/`false`
   decision before the code compiles.
