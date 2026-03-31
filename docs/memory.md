# Memory

Elle uses arena-based allocation. Values live in a fiber's arena and are
freed when the fiber completes. Scope allocation can free earlier, at
block exit.

## Arena introspection

```lisp
# current object count
(arena/count)              # => integer

# detailed stats
(def stats (arena/stats))
stats:object-count         # total live objects
stats:allocated-bytes      # bytes allocated
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

## Scope allocation

The lowerer performs escape analysis on blocks. When it determines that
allocations within a scope cannot escape, it emits `RegionEnter` /
`RegionExit` instructions that free heap objects at scope exit rather
than waiting for fiber death.

This is transparent to user code — you don't need to do anything to
benefit from it.

## Root fiber memory

The root fiber's arena persists for the program's lifetime. Child fibers
(from `ev/spawn`) get their own arenas that are freed when the fiber
completes.

---

## See also

- [fibers.md](fibers.md) — fiber lifecycle and arenas
- [runtime.md](runtime.md) — runtime signals including SIG_QUERY
- [scheduler.md](scheduler.md) — async scheduler
