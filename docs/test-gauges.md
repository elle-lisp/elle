# The test runner's heap gauges

<!-- audited: 2026-10-05 -->

What each file of an `elle test` run cost the runner's own heap, and the heaps its test code ran on.

The runner is the longest-running Elle program in this repository, and for most
of its life it measured nothing about itself. A leak of about 28000 regions per
compiled file reached us as an OOM kill of `make smoke`, and the answer was a
batch size rather than a number naming the file. The gauges turn that into a
file name and a number. [test-store](test-store.md) holds the rest of what a
run records.

## The gauges

A gauge is a primitive that answers one integer for the heap it runs on. Every
one is Immediate ([diagnostics](impl/region/diagnostics.md)), so a call
allocates nothing and cannot move the number it reports. The runner reads these,
in this order:

| Kind | Primitive |
|------|-----------|
| `objects` | `arena/count` |
| `regions` | `arena/region-count` |
| `pages` | `arena/page-claims` |
| `region-frees` | `arena/region-frees` |
| `page-frees` | `arena/page-frees` |
| `object-frees` | `arena/object-frees` |
| `one-page-frees` | `arena/one-page-frees` |
| `empty-frees` | `arena/empty-frees` |
| `one-object-frees` | `arena/one-object-frees` |
| `few-object-frees` | `arena/few-object-frees` |
| `many-object-frees` | `arena/many-object-frees` |
| `adopts` | `arena/adopts` |
| `adopts-into-empty` | `arena/adopts-into-empty` |
| `owned` | `arena/owned` |
| `owned-frees` | `arena/owned-frees` |
| `owned-free-pages` | `arena/owned-free-pages` |
| `owned-free-objects` | `arena/owned-free-objects` |
| `owned-one-page-frees` | `arena/owned-one-page-frees` |
| `rescues` | `arena/rescues` |
| `rescue-survivors` | `arena/rescue-survivors` |
| `extracts` | `arena/extracts` |
| `reparents` | `arena/reparents` |

The first three say how much a heap holds. The rest are the reclamation
counters, which say how its regions end; [diagnostics](impl/region/diagnostics.md)
defines each one. The runner keeps the list once, as `heap-gauges` in
[store.lisp](../src/test/store.lisp). The sampling, the summary and the growers
query all read that list, so a gauge added there arrives everywhere.

## Two heaps

The runner reads the same gauges on two heaps. The `heap` column of each
`gauge` row says which one:

- `runner` — the runner's own heap. What reaches it is what the runner does per
  file: the compile, the syntax it holds, and the rows it writes.
- `test` — the heaps the test code runs on. Every worker thread has its own VM
  and its own heap, so the runner cannot read one from outside. Instead each
  worker reads every gauge around its run of the form, and hands the readings
  back with the form's result.

A store written before the `heap` column existed gains it by `ALTER TABLE`. Its
rows read NULL there, which means `runner`: every row it holds was the
runner's.

### The runner heap: one reading per file boundary

The runner takes a baseline before the first file, then one reading after each
file, and charges the difference to that file. One reading per boundary is what
makes the accounting close. The rows written for one file fall inside the next
file's window, so every object the runner allocates is charged to exactly one
file. Read back, the chain is exact: a file's `reading` plus the next file's
`delta` is the next file's `reading`.

### The test heap: a difference per run, summed per file

A worker's readings bracket one run of one form on one tier, and nothing else.
The worker's own start and its scheduler fall outside them.

A reading is a loop in Elle, and the loop allocates. Between two readings of one
gauge lie the rest of the first loop and the start of the second, which together
cost one whole reading. So the worker reads three times: twice in a row, then
the call, then once more. Every reading costs the same, so the first pair
measures that cost. The runner charges the run the second difference less the
first. The runner computes the differences, so after the call the worker only
reads.

The runner adds the charges of every run of a file's forms, on every tier. It
then writes one `test` row per gauge for the file, with the sum as `delta` and
NULL as `reading`. A worker heap lives for one run, so a reading of it has
nothing to chain to.

Some runs hand back no readings, and a file records only what came back:

- A run that misses its deadline hands back nothing. The runner abandons the
  worker, and its readings with it.
- A file run under `--isolate` records no `test` row at all. The child is a
  separate process, and its heap ends with it.
- A file that fails to compile runs no form and records no `test` row.

A file whose shared setup gates is measured like any other. Its setup runs
inside its whole-file form, under each JIT policy ([test-runner](test-runner.md)),
so the gate ends a run the worker's readings already bracket. The file records
a `skip` per policy and its `test` rows.

A form that cannot cross to a worker, or whose value cannot cross back, runs in
the runner's own process instead ([test-runner](test-runner.md)). A worker run
whose value could not come back hands back nothing. The run in the runner's
process still brackets the form alone, so its readings land in the `test` rows.
The runner's window for that file contains them too.

So a file with no `test` rows was not measured, and a `test` row that reads 0
was measured and did not move.

## A file's charge, for the ratchet

`elle test --charge PATHS` runs each path in-process twice. It reads the
runner heap around the second run alone, from the moment that run starts until
its results are recorded. Each file then has three readings for the producer
`elle test`, with the path as the subject: `objects`, `regions` and `pages`
([ratchet](ratchet.md)). The first two are what the run left live, and `pages`
counts every page it claimed, freed or kept. The `gauge` rows still chain over
both runs.

The second run is the reading because a window over a file's first run holds
work that is not the file's own, and its size follows what ran before:

- A cache the runtime fills on first use is charged to the first file that
  needs it. The first file of a batch pays 23 objects more than a later run of
  itself, and a file run after another that needed the same entry pays 2
  objects and a region less.
- A window that opens at the previous boundary holds the gauge rows written
  for the previous file.
- A store that already holds a file's output skips the CAS write a fresh store
  makes, and CI starts every job with a fresh store.

The second run starts after its own first run filled the caches and the CAS,
and its window opens after every earlier recording. Its charge is the same
whatever ran before it, on a fresh store or a warm one, and on a loaded box.
A cost the file pays once per runtime, such as a cache entry keyed by the
file, falls in the first run, so the reading does not show it.

The readings are recorded against the last result of the file's second run.
A row of the file's that the run did not read is `missing`. `--charge` refuses
`--isolate`, whose child would leave the runner nothing but the spawn to
charge, and `-e`, whose form has no file to name.

`make smoke-impl` runs `--charge` on the rig, in a pass of its own, over the
language suite and the implementation files with no sidecar. A file with a
sidecar needs a mode that an in-process run cannot give it. The pass leaves
out the producers, which have their own pass, and
[config.lisp](../tests/impl/config.lisp), which asserts that a program cannot
change the JIT policy the in-process runner sets.

## Why a table of its own

A delta belongs to a file rather than to a result. A `result` column
would copy one runner number onto every row of the file, and a test-heap sum
covers every tier of every form at once. A `measurement` row is the wrong home
too: it carries a reading and the verdict it earned, and a chained delta
follows what ran before the file, so no row could judge it. A file's charge is
the reading that can be judged.

## The summary names the top growers

Every run ends with one block per heap that recorded a row. A block gives the
run's total on every gauge, then the files that grew that heap most, ranked by
regions:

```
runner heap · objects +9021 · regions +28104 · pages +112 · region-frees +40211 · …
  objects +4510  regions +14052  pages +56  region-frees +20105  …  tests/lang/a.lisp
test heap · objects +3 · regions +2 · pages +610 · region-frees +598 · adopts +2 · …
  objects +2  regions +1  pages +400  region-frees +392  adopts +2  …  tests/lang/keep.lisp
```

Each line carries every gauge in list order; the example cuts them short. The
list is a reading aid. Which file, on which commit, in which run, on which heap
is a query over `gauge`:

```sql
-- Which files cost the runner the most heap, across every run recorded.
SELECT file, sum(delta) AS regions FROM gauge
WHERE kind = 'regions' AND coalesce(heap, 'runner') = 'runner'
GROUP BY file ORDER BY regions DESC LIMIT 10;

-- Where the test code's adoptions ended, file by file, in the latest run.
SELECT file,
       sum(CASE WHEN kind = 'adopts' THEN delta END) AS adopts,
       sum(CASE WHEN kind = 'owned-frees' THEN delta END) AS owned_frees,
       sum(CASE WHEN kind = 'rescues' THEN delta END) AS rescues
FROM gauge WHERE heap = 'test' AND run_id = (SELECT max(id) FROM run)
GROUP BY file ORDER BY adopts DESC;
```
