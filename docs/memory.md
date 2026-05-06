# Memory

Elle has no garbage collector. Memory is managed deterministically through
per-fiber tracked pools, compiler-directed scope reclamation, tail-call
pool rotation, flip rotation, and deferred reference counting. These
mechanisms are derived from the same static analysis that drives signal
inference — the compiler knows at every allocation site whether the value
can escape its scope, whether the containing function is a tail call,
and whether the fiber will yield.

## How to write leak-free code

Most Elle code is naturally leak-free. The three rules:

### 1. Don't push heap values into unbounded collections in a loop

```lisp
# BAD: push stores heap structs into growing array — linear growth
(def @acc @[])
(def @i 0)
(while (< i 100)
  (push acc {:x i})
  (assign i (+ i 1)))
# the array grows without bound
```

Overwriting patterns (assign, put) are bounded thanks to deferred
reference counting — the old value is decref'd and freed at scope exit:

```lisp
# OK: assign overwrites — old value freed via refcount decrement
(def @last nil)
(def @i 0)
(while (< i 100)
  (assign last {:x i})
  (assign i (+ i 1)))
# bounded: each old struct is freed when its refcount drops to zero

# OK: put overwrites — old string freed via refcount decrement
(def @s @{:x 0})
(def @i 0)
(while (< i 100)
  (put s :x (string "v" i))
  (assign i (+ i 1)))
# bounded: each old string is freed on overwrite
```

### 2. Prefer tail calls for loops with heap allocation

```lisp
# Tail-recursive loop: trampoline rotation keeps memory bounded
(defn process-all [n]
  (if (= n 0)
    :done
    (begin
      {:x n}                       # heap allocation
      (process-all (- n 1)))))     # tail call — rotation frees {:x n}

(process-all 10000)
# memory stays bounded despite 10000 struct allocations
```

The same works for strings, mutual tail recursion, and any allocation that
doesn't outlive the iteration:

```lisp
# Mutual tail recursion — also bounded
(defn ping [n]
  (if (= n 0) :done
    (begin (string "ping " n) (pong (- n 1)))))

(defn pong [n]
  (if (= n 0) :done
    (begin (string "pong " n) (ping (- n 1)))))

(ping 10000)
```

### 3. Yielding fibers use flip rotation

Fibers that yield mid-loop cannot use scope reclamation (the fiber suspends
before `RegionExit` fires). Instead, `FlipSwap` at the loop back-edge rotates
pools each iteration:

```lisp
# Yielding fiber — flip rotation keeps memory bounded
(defn yield-items [n]
  (fiber/new (fn []
    (def @i 0)
    (while (< i n)
      (yield (string "item-" i))    # heap allocation + yield
      (assign i (+ i 1))))
  |:yield|))

(def f (yield-items 10000))
(while (not= (fiber/status f) :dead)
  (fiber/resume f))
# memory stays bounded despite 10000 string allocations across yields
```

## What is automatically reclaimed

### Scope reclamation

The compiler performs Tofte-Talpin region inference and escape analysis on
every `let`, `letrec`, and `while` body. Region inference assigns each
allocation to a lexical scope; escape analysis proves which scopes cannot leak
values (no captures, no suspension, result is immediate, no outward mutation).
Scopes that pass both checks get `RegionEnter`/`RegionExit` bytecodes.
`RegionExit` runs destructors and reclaims pool slots for objects allocated
within the scope.

```lisp
# let-bound struct is reclaimed at scope exit
(let [x {:a 1 :b 2}]
  (get x :a))                         # => 1
# x's struct is freed here

# discarded struct in while body — scope reclaims each iteration
(def @i 0)
(while (< i 1000)
  {:x i :y (+ i 1)}                   # struct allocated and discarded
  (assign i (+ i 1)))
# net allocs: ~0 (bounded by scope reclamation)
```

The escape analysis is conservative but handles common patterns:
- Discarded expressions (structs, strings, cons cells)
- `let`-bound values not captured by closures
- Closures created and called within the same scope
- `fiber/new` + `fiber/resume` within the same scope
- `protect` expressions
- `map`, `filter`, `each` over known-safe collections
- Factory-returned closures (`(def proc (make-proc))`)
- Binding aliases (`(def f existing-fn)`)
- Conditional closures (`(def f (if ... (fn ...) (fn ...)))`)

### Deferred reference counting

The slab tracks a per-slot reference count for durable references — mutable
collection entries and mutable bindings. Transient references (stack values,
let bindings, function parameters) are handled by scope marks without
refcount overhead.

When a mutable binding is overwritten (`assign`) or a mutable collection
entry is replaced (`put`), the old value's refcount is decremented. At
scope exit, `RegionExitRefcounted` skips objects whose refcount is still
positive (they're referenced by something outside the scope) and frees
everything else. This makes overwrite-heavy patterns bounded:

```lisp
# Overwrite in a loop — bounded via refcounting
(def @v (string "init"))
(def @i 0)
(while (< i 10000)
  (assign v (string "val-" i))     # old string decref'd → freed
  (assign i (+ i 1)))
# net allocs: bounded (~30)
```

### Tail-call rotation

Self-tail-calls in the trampoline get implicit pool rotation. On each tail-call
iteration, the previous iteration's allocations are moved to a swap pool and
freed on the next rotation (one-iteration lag ensures argument values remain
valid). This bounds memory at the working-set size, not the iteration count.

### Flip rotation

`while` loops inside yielding fibers get explicit `FlipEnter`/`FlipSwap`/
`FlipExit` bytecodes. Each `FlipSwap` at the back-edge rotates generations,
keeping memory bounded even when scope reclamation is blocked by yield
suspension.

### Fiber death

When a fiber completes or errors, its `FiberHeap` runs all destructors and
drops all arena pages. The fiber's entire memory footprint disappears — no
traversal, no mark phase, no sweep. A server loop spawning one fiber per
request reclaims all per-request memory at fiber death.

## How it works

Every `Value` is a 16-byte tagged union. Immediates (integers, keywords,
booleans, nil, floats) fit inline — no allocation. Heap types (strings, arrays,
structs, closures, fibers, cons cells) store a pointer to a `HeapObject` in a
tracked pool owned by the fiber.

### Per-fiber heaps

Each fiber owns a `FiberHeap` containing a `SlabPool` — a slab allocator for
HeapObjects plus a bump arena for inline slice data, both backed by `mmap`
pages. The slab allocates fixed-size HeapObject slots from chunks of 256
slots each. The bump arena allocates variable-size data (string bytes, array
elements) sequentially into 64KB pages. Both use `munmap` to return pages to
the OS on fiber death — no process-allocator caching, no RSS hoarding.

When a fiber completes, its `FiberHeap` runs all destructors, tears down all
owned shared allocators and outboxes, and returns all mmap'd pages to the OS.

### Slab allocator

The slab manages HeapObject slots:

- **Chunks**: `mmap`'d regions of 256 HeapObject slots each
- **Allocation**: check free list first, then bump cursor within last chunk
- **Deallocation**: write intrusive free-list link into the dead slot's bytes,
  return to free list for reuse
- **Reference counting**: per-slot `u32` refcount for durable references
  (mutable bindings and collection entries). `incref`/`decref` manage the
  count; `release_refcounted` skips pinned slots during scope exit
- **Pointer stability**: chunk addresses never move; `Value` payloads are
  raw pointers into chunk slots
- **OS return**: `munmap` on `Drop` returns chunk pages immediately

### Bump arena

The bump arena manages variable-size inline data (string bytes, array
elements):

- **Pages**: `mmap`'d 64KB regions — pointer-stable, never moved
- **Allocation**: bump a byte offset within the current page
- **Oversized**: allocations >64KB get dedicated pages
- **No individual deallocation**: memory is reclaimed by `release_to(mark)`
  or `clear()` on fiber death
- **OS return**: `munmap` drops pages; `madvise(MADV_DONTNEED)` on the
  retained page releases physical frames while keeping the virtual mapping

### SlabPool

`SlabPool` owns both the slab and the bump arena, plus allocation tracking:

- `allocs: Vec<*mut HeapObject>` — every allocation in order (for rotation
  and scope release)
- `dtors: Vec<*mut HeapObject>` — objects that need `Drop` (closures,
  fibers, mutable types)
- `alloc_count` — running total for `arena/count` introspection

`alloc(obj)` routes to the slab. `alloc_inline_slice(items)` routes to the
bump arena. `teardown()` clears both.

### Scope marks

`RegionEnter` pushes an `ArenaMark` recording the pool's position and destructor
count. `RegionExit` pops the mark, runs destructors for objects allocated since
the mark, and rewinds the pool. `RegionExitRefcounted` does the same but skips
objects whose refcount is positive (they're referenced by durable bindings or
collection entries). This is transparent to user code — it's entirely
compiler-directed.

### Inter-fiber value exchange

When a fiber yields a value to its parent, that value must survive the child's
death. Two mechanisms handle this, chosen at fiber creation time based on signal
inference:

**Shared allocator routing** (yielding fibers): The child fiber routes all
allocations to a `SharedAllocator` owned by the parent's `FiberHeap`. The
parent reads yielded values directly — zero copy, zero serialization.

**Outbox mechanism**: The parent installs an outbox `SlabPool` before child
execution. Between `OutboxEnter`/`OutboxExit` bytecodes, allocations go to
the outbox. At yield time, values in the private pool are deep-copied to the
outbox; values already in the outbox are returned directly. Previous outboxes
are preserved so the parent can read values from earlier yields. All outboxes
are freed in bulk on fiber death.

**Silent fibers** (no yields): neither mechanism is needed. The fiber allocates
exclusively into its own private pool with no indirection overhead.

### Ownership topology

```text
root fiber
├── private pool            ← root's own allocations (SlabPool → BumpArena pages)
├── shared allocator        ← child A's allocations (yielded values live here)
│   └── child A
│       ├── private pool    ← idle (child yields; allocations route to parent)
│       └── shared alloc    ← grandchild's allocations
│           └── grandchild
│               └── ...
└── (child B: silent)
    └── private pool        ← child B's own allocations (no sharing needed)
```

- **Silent fibers** use their private pool exclusively. Scope marks reclaim
  short-lived objects. Teardown on death reclaims everything.
- **Yielding fibers** route allocations to the parent's shared allocator (or
  outbox). Their private pool is essentially idle.
- **Parent fibers** own shared allocators in `owned_shared` and outboxes in
  `old_outboxes`. Both are torn down on `clear()`.

## Why this works without a GC

The memory model exploits two properties that the compiler guarantees:

1. **Signal inference determines ownership at creation time.** The compiler
   classifies every function as `Silent`, `Yields`, or `Polymorphic`. By the
   time a fiber is created, the runtime knows whether it will yield. This is
   the decision point for shared-allocator routing — no runtime heuristics,
   no profiling, no fallback.

2. **Tofte-Talpin region inference determines scope reclamation.** Region
   inference assigns each allocation to a lexical scope; escape analysis
   proves which scopes cannot leak values. These scopes get
   `RegionEnter`/`RegionExit` instrumentation for free.

Together, these give deterministic memory management with no GC pauses, no
write barriers, no card tables, and no stop-the-world collection. Memory is
reclaimed at five granularities: scope exit, refcount-aware scope exit,
tail-call rotation, flip rotation, and fiber death — all in bounded time.

## Introspection

```lisp
# current object count (local + shared)
(arena/count)               # => integer

# bytes committed by arena pages
(arena/bytes)               # => integer

# peak object count since last reset
(arena/peak)                # => integer

# net allocations from a thunk
(def result (arena/allocs (fn [] (pair 1 2))))
(first result)              # => (1 2)
(rest result)               # => 1

# detailed stats (returns a struct via vm/query)
(arena/stats)
# => {:object-count N :peak-count N :allocated-bytes N
#     :object-limit nil :scope-depth N :dtor-count N
#     :root-live-count N :root-alloc-count N :shared-count N
#     :active-allocator nil :scope-enter-count N :scope-dtor-count N}

# allocation limits (dangerous — for debugging only)
(arena/set-object-limit 10000)  # => previous limit or nil
(arena/object-limit)            # => 10000
(arena/set-object-limit nil)    # => 10000 (restore unlimited)

# manual checkpoint/reset (dangerous — invalidates live Values)
(def m (arena/checkpoint))
(pair 1 2)
(arena/reset m)
# the cons cell is now invalid — do not reference it
```

`arena/count` and `arena/bytes` operate directly on thread-local state with
zero allocation overhead. `arena/stats` uses a query signal so the VM can
snapshot heap state consistently.

## Measuring your code

The `arena/allocs` primitive measures net heap allocations from any expression:

```lisp
(def result (arena/allocs (fn [] (string "hello" " " "world"))))
(first result)              # => "hello world"
(rest result)               # => 1 (one string allocation)
```

For loop patterns, measure at two scales to detect linear leaks:

```lisp
(defn measure-loop [n]
  (def before (arena/count))
  (def @i 0)
  (while (< i n)
    {:x i}
    (assign i (+ i 1)))
  (- (arena/count) before))

(def d100 (measure-loop 100))
(def d10k (measure-loop 10000))

# bounded: d100 and d10k are both small, d10k is not 100x d100
(println "d100=" d100 " d10k=" d10k)
```

## Known leak patterns

These patterns leak linearly because the value genuinely escapes to an
unbounded accumulator:

| Pattern | Why it leaks |
|---------|-------------|
| `(push arr {:x i})` in a loop | Struct stored in growing array — every pushed value is kept alive |
| `(assign acc (append acc [i]))` in a loop | Functional append creates new arrays; old array freed by refcount, but the new array grows without bound |

Overwrite patterns (`assign`, `put`) are **not** leaky — deferred reference
counting frees old values at scope exit.

---

## See also

- [impl/values.md](impl/values.md) — value representation and heap types
- [signals/](signals/) — signal inference (drives scope reclamation and
  shared-allocator routing)
- [types.md](types.md) — user-facing type system
