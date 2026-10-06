# The page-claims profiler

<!-- audited: 2026-10-12 -->

A profiler that names the code paths claiming the most region pages in one run
of a program.

The `arena/page-claims` gauge counts the pages a heap claims from its page pool
([diagnostics](../../docs/impl/region/diagnostics.md)). It says how many pages a
shape costs. This tool says which code claimed them, so work that puts more
values in fewer regions can start with the code that claims the most pages.

## Running it

```sh
make page-claims ARGS="tests/lang/traits.lisp"
make page-claims ARGS="test tests/lang/traits.lisp" DEPTH=4 TOP=10
```

`ARGS` is what the profiled `elle` runs, so the second line profiles the test
runner on that file. The target builds the `profiling` profile, which keeps the
symbols callgrind reads. Then it runs [run.lisp](run.lisp) from the repository
root, because the run's directory and the default `--elle` path are relative
to it. Valgrind must be installed.

`DEPTH` sets how many frames a path holds, 3 unless set. `TOP` sets how many
paths print, 20 unless set. The tool prints the total of page claims, then one
line for each path: its claims, then the path. The path with the most claims
comes first, and a tie goes to the path whose text sorts first. So two runs of
one program print their paths in one order.

When the profiled program exits with a failure, the tool says so and still
ranks the claims it made. When callgrind writes no output, the tool stops with
an error.

## What a path is

`RegionPool::add_page` is the one function that claims a page for a region
([regionpool.rs](../../src/value/fiberheap/regionpool.rs)). The tool runs
callgrind with `--separate-callers12=*add_page*`, so callgrind keeps a separate
call count for each chain of up to 12 callers of `add_page`.
[rank.lisp](rank.lisp) sums those counts by path.

A path is the first `DEPTH` frames of a chain that are not plumbing, innermost
first, joined by ` <- `, without the `elle::` prefix. A plumbing frame only
moves an allocation toward a page. The plumbing is the region store and the
page pool, the heap's allocation entry points, the `Alloc` capability, and the
value builders. So two builders that one caller allocates through share a path,
and a path names the code that chose to allocate. Chains that differ only past
the path's last frame add into one path.

A chain holds at most 12 frames. When plumbing fills most of a chain, its path
holds fewer frames than `DEPTH`.

## A stdlib cache of its own

The profiled `elle` reads the standard library from a cache of its own. Before
callgrind runs, the tool fills that cache by running an empty program. So the
claims are those of a warm start, and two runs of one program rank alike.

The shared cache would not do. Each `elle` binary that stores to a cache
directory removes the files of the other binaries
([stdlib-cache](../../docs/impl/stdlib-cache.md)). In that cache, the profiled
run would compile the standard library, and claim its pages, only when a
different binary ran last.

## Where the files go

One run keeps its files in `target/page-claims/PID`, where `PID` is the process
id of the tool. The directory holds the cache, the empty program and the
callgrind output, and the tool deletes it when the run ends, on success or on
failure. The `elle` that runs the tool uses no stdlib cache, so it compiles
the standard library itself. So the tool writes nothing outside `target/`, and
two runs at once keep their files apart.
