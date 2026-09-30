# The test runner's heap gauges

<!-- audited: 2026-09-29 -->

What each file of an `elle test` run cost the runner's own heap, and the heaps its test code ran on.

The runner is the longest-running Elle program in this repository, and for most
of its life it measured nothing about itself. A leak of about 28000 regions per
compiled file reached us as an OOM kill of `make smoke`, and the answer was a
batch size rather than a number naming the file. The gauges turn that into a
file name and a number. [test-store](test-store.md) holds the rest of what a
run records.

## The gauges

A gauge is a primitive that answers one integer for the heap it runs on. Every
one is Immediate ([diagnostics](impl/region/diagnostics.md)), so a reading
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
  worker reads every gauge right before its form's tiered call and right after
  it, and hands the differences back with the form's result.

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
The worker's own start and its scheduler fall outside them. The runner adds the
differences of every run of a file's forms, on every tier. It then writes one
`test` row per gauge for the file, with the sum as `delta` and NULL as
`reading`. A worker heap lives for one run, so a reading of it has nothing to
chain to.

Some runs hand back no difference, and a file records only what came back:

- A run that misses its deadline hands back nothing. The runner abandons the
  worker, and its readings with it.
- A file run under `--isolate` records no `test` row at all. The child is a
  separate process, and its heap ends with it.
- A file that fails to compile, or that gates in its shared setup, runs no form
  and records no `test` row.

A form whose closure cannot be sent to a worker runs in the runner's own
process instead ([test-runner](test-runner.md)). Its readings still bracket the
form alone, so they land in the `test` rows. The runner's window for that file
contains them too.

So a file with no `test` rows was not measured, and a `test` row that reads 0
was measured and did not move.

## Why a table of its own

A delta belongs to a file rather than to a result. A `result` column
would copy one runner number onto every row of the file, and a test-heap sum
covers every tier of every form at once. A `measurement` row is the wrong home
too: it carries a dashboard's verdict off the channel of an isolated child, and
a per-file delta has no verdict to give.

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
