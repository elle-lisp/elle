# Memory

Elle has no garbage collector. Memory is managed deterministically through
per-fiber bump arenas, escape-analysis-driven scope reclamation, and
zero-copy inter-fiber sharing. These three mechanisms are derived from the
same static analysis that drives the signal system — signal inference tells
the runtime which fibers yield and which are silent, and that distinction
determines how memory is allocated, shared, and reclaimed.

## How it works

Every `Value` is a 16-byte tagged union. Immediates (integers, keywords,
booleans, nil) fit inline — no allocation. Heap types (strings, arrays,
structs, closures, fibers) store a pointer to a `HeapObject` in a bump
arena owned by the fiber.

### Per-fiber heaps

Each fiber owns a `FiberHeap` containing a `SlabPool` — a bump arena
with destructor tracking. The bump arena allocates sequentially into
64KB pages. There is no per-slot free list; memory is reclaimed only at
scope boundaries or fiber death.

When a fiber completes, its `FiberHeap` runs all destructors and drops
all arena pages. The fiber's entire memory footprint disappears — no
traversal, no mark phase, no sweep. A server loop spawning one fiber per
request reclaims all per-request memory at fiber death.

### Bump arena

The `BumpArena` is a byte-level sequential allocator:

- **Pages**: `Vec<Box<[MaybeUninit<u8>; 64KB]>>` — pointer-stable
- **Allocation**: bump a byte offset within the current page
- **Oversized**: allocations >64KB get dedicated pages
- **No individual deallocation**: `dealloc_slot()` is a no-op
- **Reclamation**: `release_to(mark)` truncates pages and resets offset;
  `clear()` on fiber death resets entirely (keeps first page for reuse)

This gives cache-friendly sequential allocation, zero fragmentation, and
deterministic bulk reclamation.

### Scope reclamation

The lowerer performs escape analysis on every `let`, `letrec`, and `block`
scope. When it can prove that no allocated value escapes — no captures, no
suspension, result is immediate, no outward mutation — it emits
`RegionEnter` / `RegionExit` bytecodes that reclaim heap objects at scope
exit rather than waiting for fiber death.

`RegionEnter` pushes an `ArenaMark` recording the arena's page/offset and
destructor count. `RegionExit` pops the mark, runs destructors for objects
allocated since the mark, and rewinds the arena to the mark position. This
is transparent to user code.

### Zero-copy inter-fiber sharing

When a fiber yields a value to its parent, that value must survive the
child's death. Copying is expensive and breaks identity. Instead, the
runtime uses signal inference to solve this at the allocation level.

The compiler knows at fiber-creation time whether a fiber can yield
(its signal includes `SIG_YIELD`). For yielding fibers, the runtime
installs a `SharedAllocator` owned by the parent's `FiberHeap`. While the
child executes, **all** of its allocations route to this shared arena — not
selectively, not per-scope. The parent reads yielded values directly from
shared memory: zero copy, zero serialization.

For silent fibers (no yields), the shared allocator is never installed.
The fiber allocates exclusively into its own private arena with no
indirection overhead.

### Outbox

For yield-safe allocation, the runtime uses an outbox mechanism:

- Parent installs an outbox (`Box<SlabPool>`) before child execution
- Child allocates between `OutboxEnter`/`OutboxExit` bytecodes into the
  outbox arena
- At yield time, parent reads the outbox and stores it
- On fiber death, all outboxes are freed in bulk

This ensures yielded values don't reference the child's private heap.

### Ownership topology

```text
root fiber
├── private arena        ← root's own allocations (BumpArena pages)
├── shared allocator     ← child A's allocations (yielded values live here)
│   └── child A
│       ├── private arena  ← idle (child yields, so everything routes to parent)
│       ├── outbox         ← yield-bound allocations
│       └── shared alloc   ← grandchild's allocations
│           └── grandchild
│               └── ...
└── (child B: silent)
    └── private arena    ← child B's own allocations (no sharing needed)
```

- **Silent fibers** use their private arena exclusively. Scope marks
  reclaim short-lived objects. `clear()` on death reclaims everything.
- **Yielding fibers** route all allocations to the parent's shared arena.
  Their private arena is idle.
- **Parent fibers** own shared allocators in `owned_shared`. The shared
  allocator has its own bump arena and destructor tracking.

## Why this works without a GC

The memory model exploits two properties that the compiler guarantees:

1. **Signal inference determines ownership at creation time.** The compiler
   classifies every function as `Silent`, `Yields`, or `Polymorphic`. By the
   time a fiber is created, the runtime knows whether it will yield. This is
   the decision point for shared-allocator routing — no runtime heuristics,
   no profiling, no fallback.

2. **Escape analysis determines scope reclamation.** The compiler's capture
   analysis and suspension tracking prove which scopes cannot leak values.
   These scopes get `RegionEnter`/`RegionExit` instrumentation for free.

Together, these give deterministic memory management with no GC pauses, no
write barriers, no card tables, and no stop-the-world collection. Memory
is reclaimed at three granularities: scope exit, fiber death, and shared
allocator teardown — all in bounded time.

## Introspection

```lisp
# current object count
(arena/count)              # => integer

# detailed stats
(def stats (arena/stats))
stats:object-count         # total live objects
stats:allocated-bytes      # bytes committed by arena pages
```

`arena/count` operates directly on thread-local state with zero
allocation overhead.
