# The test runner store

<!-- audited: 2026-09-20 -->

Where `elle test` keeps a run, what every run and result records, and the
queries that read them back.

How a run executes is [test-runner](test-runner.md); why the runner exists
and how to drive it is [test-cli](test-cli.md).

## Architecture: the corpus is text, the database is a derived index

What feels wrong about "tests in SQLite" is real: a binary database has no
meaningful diffs, merge-conflicts badly, grows without bound, and distributes as
a blob. The fix is not a different store — it is a **one-way authority arrow**.
The database is never authoritative. Two layers:

- **The corpus is text, and lives in git.** A test is a *form*, identified by
  the hash of its syntax. Durable forms are forms in `.lisp` files, exactly as
  today — diffable, reviewable, distributed by `git clone`. This is the only
  source of truth, and it is plain text.
- **The database is a local, derived index — the history.** It lives in the
  state directory (below), outside the repo. It persists across many
  `elle test` invocations, accumulating run history. It holds the `form` rows
  (scanned from the corpus), the run log, results, and metadata. Authority flows
  corpus → index, never the reverse.

So git sees only text you can read; the rich queryable data is a cache that
*happens* to give you SQL joins. Distribution is `git clone` — each machine
builds its own index on first run. Sharing a *results set* (for example CI
publishing for an agent to inspect) is an optional `tar` of the index + CAS, and
it never touches the repo.

### Run history is state, not cache

A cache is what a rebuild can regenerate: the stdlib disk cache and the boot
image both qualify, and `ELLE_CACHE` is where they belong. A run is a record of
something that happened once, on one commit, on one machine. Nothing
regenerates it, so it lives in a state directory instead — on a development box
`ELLE_CACHE` is often a tmpfs, and a reboot there erases every run ever
recorded.

The runner takes the first of these that names a directory:

| Source | Location |
|--------|----------|
| `--db PATH` | that file, and its directory holds the CAS and the scratch files |
| `ELLE_STATE` | `$ELLE_STATE/elle-tests.db` |
| `XDG_STATE_HOME` | `$XDG_STATE_HOME/elle/elle-tests.db` |
| `HOME` | `$HOME/.local/state/elle/elle-tests.db` |
| `ELLE_CACHE` | `$ELLE_CACHE/elle-tests.db` |
| none of them | `target/elle-tests.db` |

An empty variable counts as unset. The CAS and the scratch directory are
siblings of the database file — `<db-dir>/cas` and `<db-dir>/scratch` — so
`--db` moves the whole store, which is what an isolated test run wants.

### Ad-hoc tests: born at the prompt, promotable to the corpus

An agent rarely starts with a file; it probes: `elle test -e '(assert (= (foo)
42))'`. That form runs like any other and is **persisted into the index as an
ad-hoc form** — same syntax-hash identity, tagged `origin=:adhoc` and stamped
with the session. For the rest of the session it is part of the suite: a plain
`elle test` re-runs it, `--rerun-failed` includes it, every query sees it. It is
*not* in git — it has no file.

When an ad-hoc test earns its keep, `elle test --promote <id> [file]` renders its
syntax to text and appends it to a `.lisp` file. Now it is durable: in git,
diffable, distributed, and re-derived as a durable form on the next scan. The
motion from throwaway probe to permanent regression test is one command, and
identity is preserved across it because both sides key on the same syntax hash.

Ad-hoc forms vanish when `--prune adhoc` clears them. Either way they are
ephemeral by construction, while durable forms always re-derive from the
corpus.

### The durable corpus is a flat set, classified by query

Tests are stored **one form per file** — file == test == syntax hash —
addressable, with their own blame and history, movable and promotable as atoms.
There is **no directory hierarchy**, and filesystem order is treated as
**semantically void**. This is the same argument made twice:

- *Order* is not a property of an isolated test. The runner owns execution order
  — by hash, by failure-recency, or randomized to surface hidden inter-test
  coupling — and records the order it used, so an order-dependent failure
  reproduces. Nothing about sequence belongs in a path.
- *Category* is not single-valued either. A form that does `(chan/send …)`
  inside `(fiber/new …)` while catching a signal is a channel test *and* a fiber
  test *and* a signal test. A directory forces one bucket and discards the rest;
  `compile/analyze` already yields the faceted truth (`touches`, `caps`, signal
  profile), so any grouping you'd draw as a tree is a `SELECT` over the index and
  a form appears in *every* view that applies. **Category is a query, not a
  directory.**

What promotion assigns is therefore a **name**, not a **place**: `touches
chan/send` + the derived label → `bounded-send-full.lisp`. A human *reads* the
failing test, and that legibility is the one need the index can't derive away —
order and hierarchy both can. Names are suggested from analysis and confirmed
with context at promotion.

Directories can be added later as a thin, non-authoritative reading-aid for
humans browsing the repo without the index handy — they never become the source
of classification, and the runner never depends on them.

The runner compiles any file regardless of how many forms it holds (via the
multi-form compilation mode, [test-runner](test-runner.md)), so today's multi-form
`tests/elle/*.lisp` keep working unchanged; exploding them into the
one-form-per-file shape is a mechanical codemod for when it's convenient, not a
prerequisite.

## What gets captured

Per **run** (one `elle test` invocation): wall time, peak RSS, user/sys CPU
(`getrusage`), the `HEAD` commit, whether the working tree is dirty, a tree hash,
the worktree the run ran in, the elle build version/profile/host, the full
`argv`, the tier set, and the working-tree files that differ from `HEAD` with
their content hashes (the "hash of changed files").

The code-state columns are what makes a result belong to something. Without
them a row says a form failed and cannot say against which commit, on which
machine, or in which checkout — and the run history holds every worktree on the
box, so the killed-run warning cannot tell this checkout's runs from a
sibling's. The runner reads them from `git` at insert:

- `git_commit` — `git rev-parse HEAD`, and `git_dirty` — whether
  `git status --porcelain` printed anything.
- `tree_hash` — one `git hash-object` over the `HEAD` tree, the porcelain
  status, and the diff from `HEAD`. Two runs share it when they ran against the
  same code, dirty working tree included.
- `worktree` — `git rev-parse --show-toplevel`, the checkout the run ran in.

Outside a repository each of those is NULL, which is the honest answer: the run
happened, and nothing names the code it ran against.

Per **(form × tier)**: status, reason, expected/actual and predicate syntax
(from the `assert` macro, [test-runner](test-runner.md)), the emitted signal on failure, wall time,
and **CPU time** — the delta of `(clock/cpu)` read across the form's evaluation.

> CPU delta, not fuel. Fuel (`SIG_FUEL`) is specific to the `std/process`
> scheduler, is not consumed by Elle's default root scheduler, and essentially
> counts continuation-passing rather than CPU work — so it does not capture
> cost. A `(clock/cpu)` before/after delta does. It is not bit-for-bit
> deterministic, so regression queries on it compare distributions/thresholds,
> not exact equality; for that, prefer many runs (which we keep) over one.

Per **asset**: for every (form × tier) the runner captures the full `--dump`
artifact set (`ast, fhir, defuse, regions, escape, hir, lir, cfg, dfa, jit`),
`--stats`, and stdout/stderr — written to the filesystem CAS
([test-runner](test-runner.md)), deduped by hash, so identical artifacts across
runs and tiers cost one file.
`--trace` is captured **only for failing/diverging forms** (too large for the
always-set), likewise to the CAS. History is **kept indefinitely**; pruning is
explicit only (`elle test --prune`), except ad-hoc forms (§ Ad-hoc tests).

> **Temporarily**, the `--dump` artifact set is **not** captured
> ([test-runner](test-runner.md) § CAS asset capture) — it OOMs the corpus run
> and does not dedup. stdout/stderr are still captured per (form × tier);
> `--dump` capture returns once the region leak it exposes is fixed.

## Measurements: a verdict a query can read

A dashboard measures a rate and prints it. Printed, a rate is prose: nothing
can ask what it was three commits ago, and nothing can check the dashboard's
coverage against a declared set. So a dashboard also *reports* each verdict,
and the runner records it.

The channel is a file, and `ELLE_TEST_MEASUREMENTS` names it in the child's
environment. The dashboard appends one JSON object per verdict:

```json
{"subject":"io-drop","axis":"regions","value":0.0,"unit":"regions/op","verdict":"closed"}
```

Unset, the channel is closed and the dashboard writes nothing — so a direct
`elle tests/elle/oracle.lisp` run reads exactly as it read before, and the
stdout rendering stays the human's copy. The runner names the file for each
`--isolate` child ([test-runner](test-runner.md)), reads it once the child
exits, and writes one `measurement` row per line against that child's result.
The child process is what makes the variable safe to set: the environment is
process-global, so a per-form value would race between workers sharing one.

The axis is a property of the instrument rather than of the probe. A gauge in
[estimator.lisp](../tests/elle/lib/estimator.lisp) names the dimension it reads
and the unit a rate on it carries, and every probe already hands the estimator
its gauge — so no probe declares an axis and none can declare the wrong one.
The subject is the probe's label with the `label@axis` display suffix removed,
so one probe read on two dimensions is one subject and two axes.

The verdict recorded is the one displayed: a by-design growth probe reads
`growth`, so `open` in this table means a defect, exactly as it does on the
dashboard.

A run that recorded any measurement says so, and names the ones that are
neither `closed` nor `growth` — the two verdicts that are the expected answer:

```
3 measurements · 1 open · 2 closed
  open  tests/elle/oracle.lisp  reduce  objects  1.002 objects/op
```

The rest is a query. The summary is a reading aid, and every number in it comes
out of the table.

## The runner's own gauges

The runner is the longest-running Elle program in this repository, and for most
of its life it measured nothing about itself. A leak of about 28000 regions per
compiled file reached us as an OOM kill of `make smoke`, and the answer was a
batch size rather than a number naming the file.

So the runner reads three gauges of its own heap — `arena/count` for objects,
`arena/region-count` for regions, and `arena/page-claims` for pages — and
records what each file cost it. All three are Immediate primitives
([diagnostics](impl/region/diagnostics.md)), so a reading allocates nothing and
cannot move the number it reports.

### One reading per file boundary

The runner takes a baseline before the first file, then one reading after each
file, and charges the difference to that file. One reading per boundary is what
makes the accounting close. The rows written for one file fall inside the next
file's window, so every object the runner allocates is charged to exactly one
file. Read back, the chain is exact: a file's `reading` plus the next file's
`delta` is the next file's `reading`.

A reading covers the runner's own heap and nothing else. Every worker thread
has its own VM and its own heap, and an `--isolate` child is a whole separate
process, so what the test code allocates never reaches these numbers. What
reaches them is what the runner does per file: the compile, the syntax it
holds, and the rows it writes.

### Why a table of its own

A delta belongs to the window between two boundaries rather than to a
(form × tier), so a `result` column would copy one number onto every row of the
file. A `measurement` row is the wrong home too: it carries a dashboard's
verdict off the channel of an isolated child, and a per-file delta has no
verdict to give.

### The summary names the top growers

Every run ends with the totals and the files that grew the heap most, ranked by
regions:

```
runner heap · objects +9021 · regions +28104 · pages +112
  objects +4510  regions +14052  pages +56  tests/elle/a.lisp
  objects +4511  regions +14052  pages +56  tests/elle/b.lisp
```

The list is a reading aid. Which file, on which commit, in which run is a query
over `gauge`.

## Schema

```sql
CREATE TABLE run (                  -- one row per `elle test` invocation
  id INTEGER PRIMARY KEY, started_at TEXT,
  finished_at TEXT,                 -- stamped at completion; NULL = the run was KILLED mid-flight
  git_commit TEXT, git_dirty INT, tree_hash TEXT, worktree TEXT,  -- the code state this run ran against
  elle_version TEXT, build_profile TEXT, host TEXT, argv TEXT, tiers TEXT,
  selection TEXT,                   -- the filter predicate; NULL = full run (the gate)
  n_selected INT,                   -- files + -e forms planned; written at insert
  n_pass INT, n_fail INT, n_skip INT, n_diverge INT, n_timeout INT,  -- aggregated at completion only
  wall_ms INT, max_rss_kb INT, cpu_user_ms INT, cpu_sys_ms INT);   -- resource usage (v1: deferred)

CREATE TABLE changed_file (         -- working tree vs HEAD at run time
  run_id INT REFERENCES run(id), path TEXT, status TEXT, blob_hash TEXT);

CREATE TABLE form (                 -- deduped across runs; the computer names it
  hash TEXT PRIMARY KEY,            -- hash of the read Syntax (comments elided)
  origin TEXT,                      -- a .lisp path (durable, in git), or ':adhoc'
  session TEXT,                     -- session id for ad-hoc forms; NULL for durable
  file TEXT, form_index INT, line INT, col INT,
  label TEXT,                       -- derived: assert message / leading symbols
  src TEXT,                         -- the form's syntax, rendered for display
  caps TEXT, touches TEXT, signal TEXT);   -- from compile/analyze — drives selection

CREATE TABLE result (               -- one row per (form × tier × run)
  id INTEGER PRIMARY KEY, run_id INT REFERENCES run(id),
  form_hash TEXT REFERENCES form(hash), tier TEXT,
  status TEXT,                      -- pass|fail|skip|diverge|error
  reason TEXT, expected TEXT, actual TEXT, syntax TEXT, signal TEXT,
  wall_ms INT, cpu_us INT);       -- cpu_us = (clock/cpu) delta across the form

CREATE TABLE asset (                -- artifact attached to a result; bytes live in the CAS
  result_id INT REFERENCES result(id),
  kind TEXT,                        -- ast|fhir|hir|lir|cfg|dfa|jit|stats|stdout|stderr|trace
  hash TEXT, size INT, codec TEXT); -- bytes at <db-dir>/cas/<hash>; codec e.g. zstd

CREATE TABLE measurement (          -- one dashboard verdict, reported through the channel
  run_id INT REFERENCES run(id),
  result_id INT REFERENCES result(id),  -- the child whose channel carried it
  subject TEXT, axis TEXT,          -- the probe, and the dimension it was read on
  value REAL, unit TEXT,            -- the rate, and what one unit of it is
  verdict TEXT);                    -- closed|open|growth|inconclusive|contaminated

CREATE TABLE gauge (                -- what one file cost the runner's own heap
  id INTEGER PRIMARY KEY,           -- insertion order, which is boundary order
  run_id INT REFERENCES run(id),
  file TEXT,                        -- the file this boundary's window is charged to
  kind TEXT,                        -- objects|regions|pages
  delta INT,                        -- the change since the previous boundary
  reading INT);                     -- the gauge at this boundary
```

The runner writes this with `lib/sqlite.lisp` (FFI to libsqlite3). The DB holds
only metadata and hashes; artifact bytes live in the on-disk CAS, so the file
stays small and merge/diff concerns never arise (it is gitignored regardless).

**v1 implemented subset ([store.lisp](../src/test/store.lisp) `ensure-schema`).**
The runner creates
`form`, `result`, `asset`, `measurement` and `gauge` with the columns above;
`run` and `changed_file` are subsets:

- `run` carries every column above except the resource ones
  (`wall_ms`/`max_rss_kb`/`cpu_user_ms`/`cpu_sys_ms`), which are deferred. So a
  resource query is design-only until they land; a `SELECT` of a deferred
  column errors with `no such column`. A session DB written before the
  code-state columns existed gains them by `ALTER TABLE`, with NULL for every
  run recorded until then.
- `changed_file` is created but never populated (no `--changed` capture yet).

## The agent workflow, as SQL

Everything below is one query against the session DB (§ Run history is state).
None of it re-runs the suite.

```sql
-- What failed, completely, in the latest run — full detail, immune to truncation.
SELECT f.file, f.line, f.label, r.tier, r.expected, r.actual, r.signal
FROM result r JOIN form f ON f.hash = r.form_hash
WHERE r.run_id = (SELECT max(id) FROM run) AND r.status = 'fail';

-- Locate the LIR for a failing form WITHOUT re-running `elle --dump=lir`.
-- Returns the CAS hash; read the (compressed) bytes from <db-dir>/cas/<hash>.
SELECT hash, codec FROM asset WHERE result_id = ? AND kind = 'lir';

-- Regression archaeology: when did this form first start failing?
SELECT min(run.git_commit) FROM result JOIN run ON run.id = result.run_id
WHERE result.form_hash = ? AND result.status = 'fail';

-- Perf drift: forms whose CPU time rose materially vs a baseline run.
SELECT cur.form_hash, cur.tier, base.cpu_us AS was, cur.cpu_us AS now
FROM result cur JOIN result base
  ON base.form_hash = cur.form_hash AND base.tier = cur.tier
WHERE cur.run_id = ? AND base.run_id = ? AND cur.cpu_us > base.cpu_us * 2;

-- One leak rate's history across commits, which no printed dashboard can give.
SELECT run.git_commit AS sha, m.value AS rate, m.unit AS unit, m.verdict AS verdict
FROM measurement m JOIN run ON run.id = m.run_id
WHERE m.subject = 'io-drop' AND m.axis = 'regions' ORDER BY m.run_id;

-- Which files cost the runner the most heap, across every run recorded.
SELECT file, sum(delta) AS regions FROM gauge
WHERE kind = 'regions' GROUP BY file ORDER BY regions DESC LIMIT 10;
```

