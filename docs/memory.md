# Memory

Elle has no garbage collector. Memory is managed deterministically through
per-fiber heaps, escape-analysis-driven scope reclamation, and zero-copy
inter-fiber sharing. These three mechanisms are derived from the same static
analysis that drives the signal system — signal inference tells the runtime
which fibers yield and which are silent, and that distinction determines how
memory is allocated, shared, and reclaimed.

## How it works

Every `Value` is a 16-byte tagged union. Immediates (integers, keywords,
booleans, nil) fit inline — no allocation. Heap types (strings, arrays,
structs, closures, fibers) store a raw pointer to a `HeapObject` in a
slab allocator owned by the fiber.

### Per-fiber heaps

Each fiber owns a `FiberHeap` containing a `RootSlab` — a chunk-based
typed slab allocator. Each chunk holds 256 `HeapObject` slots. Freed slots
return to an intrusive free list and are reused by the next allocation.

When a fiber completes, its `FiberHeap` runs all destructors and drops
all slab chunks. The fiber's entire memory footprint disappears — no
traversal, no mark phase, no sweep. A server loop spawning one fiber per
request reclaims all per-request memory at fiber death.

### Scope reclamation

The lowerer performs escape analysis on every `let`, `letrec`, and `block`
scope. When it can prove that no allocated value escapes — no captures, no
suspension, result is immediate, no outward mutation — it emits
`RegionEnter` / `RegionExit` bytecodes that reclaim heap objects at scope
exit rather than waiting for fiber death.

`RegionEnter` pushes a mark recording the slab position. `RegionExit`
pops the mark, runs destructors for objects allocated since the mark,
and returns their slab slots to the free list. This is transparent to
user code — you don't need to do anything to benefit from it.

### Zero-copy inter-fiber sharing

When a fiber yields a value to its parent, that value must survive the
child's death. Copying is expensive and breaks identity. Instead, the
runtime uses signal inference to solve this at the allocation level.

The compiler knows at fiber-creation time whether a fiber can yield
(its signal includes `SIG_YIELD`). For yielding fibers, the runtime
installs a `SharedAllocator` owned by the parent's `FiberHeap`. While the
child executes, **all** of its allocations route to this shared slab — not
selectively, not per-scope. The parent reads yielded values directly from
shared memory: zero copy, zero serialization.

For silent fibers (no yields), the shared allocator is never installed.
The fiber allocates exclusively into its own private slab with no
indirection overhead.

### Ownership topology

The result is a specific ownership structure:

```text
root fiber
├── private slab          ← root's own allocations
├── shared allocator      ← child A's allocations (yielded values live here)
│   └── child A
│       ├── private slab  ← idle (child yields, so everything routes to parent)
│       └── shared alloc  ← grandchild's allocations
│           └── grandchild
│               └── ...
└── (child B: silent)
    └── private slab      ← child B's own allocations (no sharing needed)
```

- **Silent fibers** use their private slab exclusively. Scope marks reclaim
  short-lived objects. `clear()` on death reclaims everything.
- **Yielding fibers** route all allocations to the parent's shared slab.
  Their private slab is idle.
- **Parent fibers** own shared allocators in `owned_shared`. The shared
  allocator is itself a `RootSlab` with its own destructor and scope-mark
  tracking.

This is why a per-fiber-tree shared arena wouldn't work: `clear()` is
per-fiber lifecycle. When a child dies, its temporaries are reclaimed
immediately. A shared arena would accumulate dead children's garbage
until the root clears — fatal for long-running servers.

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
stats:allocated-bytes      # bytes committed by slab chunks
```

`arena/count` operates directly on thread-local state with zero
allocation overhead.

## Measuring allocations

`arena/allocs` runs a thunk and returns `[result net-allocations]`:

```lisp
(def result (arena/allocs (fn [] (list 1 2 3))))
(first result)             # => (1 2 3) — the thunk's return value
(rest result)              # => (3) — net allocations
```

### Allocation costs by type

```text
Type              Heap objects
──────────────────────────────
(cons a b)        1
(list 1 2 3 4 5)  5 (one cons per element)
(fn [x] x)        1 (closure)
[1 2 3]           1 (array)
{:a 1 :b 2}      1 (struct)
42                0 (immediate — no heap)
:keyword          0 (immediate)
nil               0 (immediate)
```

Each heap object is a `HeapObject` — a fixed-size slot in the slab. The
slot may internally contain `Vec`, `BTreeMap`, `Box<str>`, or other Rust
heap data; `needs_drop()` tracks which variants need destructor calls.

---

## See also

- [fibers.md](fibers.md) — fiber lifecycle
- [runtime.md](runtime.md) — runtime signals including SIG_QUERY
- [scheduler.md](scheduler.md) — async scheduler
- [impl/values.md](impl/values.md) — value encoding and heap object layout
