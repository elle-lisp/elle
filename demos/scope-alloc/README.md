# Scope-Based Allocation Demo

<!-- audited: 2026-09-26 -->

A workload that allocates in tight loops inside child fibers and prints how many
heap objects each loop leaves alive.

## What it measures today

[scope-alloc.lisp](scope-alloc.lisp) runs five loop shapes of 10,000 iterations
each — a `let` returning `(length data)`, a `let` returning an outer binding,
nested `let`s reducing to arithmetic, a `match` returning keywords, and an
outward `assign` of an immediate — plus one fiber that runs them all. Each loop
runs in a non-yielding child fiber, and the demo prints the change in
`arena/count` across it.

Every shape now leaves no object alive: region inference releases each loop's
temporaries on the iteration that made them
([docs/regions.md](../../docs/regions.md)). The "unscoped" comparison in tier
1, which stores each array in an outer `var`, reads 0 as well, because the
displaced array is released at the store.

The demo was written for the earlier scope allocator, which counted region
enters and destructor runs. `arena/stats` no longer carries those counts, so
the `enters` and `dtors-run` lines print `nil`, and the combined line prints
the whole `arena/stats` struct instead.

## Running the demo

```bash
cargo run --release -- demos/scope-alloc/scope-alloc.lisp
```

`--dump=stats` adds the exit-time statistics on stderr: the JIT's compiled and
rejected functions, the page-claim histogram, and the regions the teardown
left alive ([docs/config.md](../../docs/config.md) § Statistics).

```bash
cargo run --release -- --dump=stats demos/scope-alloc/scope-alloc.lisp
```
