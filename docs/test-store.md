# The test runner store

<!-- audited: 2026-10-04 -->

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

### A run from another store joins by import

A run recorded on another box arrives as a file: the session DB, and the CAS
beside it. `elle test --import PATH` merges that store into the local one, so a
downloaded run answers the same queries a local run answers. How to fetch one
from a CI job is [ci](analysis/ci.md).

PATH names the foreign database, and its CAS is the `cas` directory beside it —
the layout `--db` already makes. What merges follows what each table is keyed
by:

- `form` rows are keyed by syntax hash, so a form both stores hold is one row.
- `run` rows append with their identity columns intact, so an imported row
  still names the commit, worktree, host and build it ran against.
- `result`, `asset`, `measurement`, `gauge` and `changed_file` rows follow
  their run, and each result id is remapped as the row lands.
- CAS files are copied by address, so bytes both stores hold are stored once.
  An asset whose bytes the foreign CAS does not carry still imports: the row
  says what was captured, and the address reads once the bytes arrive.

One run is one transaction. An import that dies partway therefore leaves the
runs it had finished and nothing half-written, which matters because the key
below makes a second import skip the runs the first one landed.

#### The run key

A run's `id` is a row number, and every store mints the same numbers, so a
second import of one artifact would append every run again. The runner gives
each run a `run_key` at insert — the host, the process, the instant and the
argv, hashed — and the key travels with the row. A unique index on it makes a
repeated import a no-op, and makes importing a store into itself do nothing.

A run recorded before the key existed carries none. The import derives one for
that row from its id, its start, its host, its worktree and its argv, so
repeating that import is a no-op as well.

### Ad-hoc tests: born at the prompt, promotable to the corpus

An agent rarely starts with a file; it probes: `elle test -e '(assert (= (foo)
42))'`. That form runs like any other and is **persisted into the index as an
ad-hoc form** — same syntax-hash identity, with `origin` set to `:adhoc`. Every
query sees it. It is *not* in git — it has no file, so no later run scans it
again.

When an ad-hoc test earns its keep, `elle test --promote <hash> <name>` renders
its syntax into `<corpus>/<name>.lisp`. Now it is durable: in git, diffable,
distributed, and re-derived as a durable form on the next scan. The motion from
throwaway probe to permanent regression test is one command, and identity is
preserved across it because both sides key on the same syntax hash.

An ad-hoc form stays in the index until `elle test --reset` removes the whole
store. Durable forms always re-derive from the corpus.

### The durable corpus is a flat set, classified by query

The design stores tests **one form per file** — file == test == syntax hash —
addressable, with their own blame and history, movable and promotable as atoms.
There is **no directory hierarchy**, and filesystem order is treated as
**semantically void**. This is the same argument made twice:

- *Order* is not a property of an isolated test. Execution order belongs to the
  runner — by hash, by failure-recency, or randomized to surface hidden
  inter-test coupling. Nothing about sequence belongs in a path.
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
order and hierarchy both can. `--promote` takes the name from its caller;
suggesting one from analysis is design ([test-cli](test-cli.md)).

Directories can be added later as a thin, non-authoritative reading-aid for
humans browsing the repo without the index handy — they never become the source
of classification, and the runner never depends on them. The split between
`tests/lang/` and `tests/impl/` is not a category in this sense: it says which
suite owns a claim and which program runs it ([spec](spec.md)),
which no analysis of the form can derive.

The runner compiles any file regardless of how many forms it holds (via the
multi-form compilation mode, [test-runner](test-runner.md)), so the multi-form
files in both suites run unchanged; exploding them into the
one-form-per-file shape is a mechanical codemod for when it's convenient, not a
prerequisite.

## What gets captured

Per **run** (one `elle test` invocation): the `HEAD` commit, whether the working
tree is dirty, a tree hash, the worktree the run ran in, the elle build
version/profile/host, the build's key (`build`: what the rig's `(elle/build)`
answers when the runner runs as `elle-rig test`, and NULL otherwise; a ledger
row belongs to one build, [ratchet](ratchet.md)), the runner's process id, the
boot fingerprint (§ The boot fingerprint), the full `argv`, and where its
results ran (`tiers`, [test-runner](test-runner.md)). The design adds wall time, peak RSS and
user/sys CPU (`getrusage`), and the working-tree files that differ from `HEAD`
with their content hashes; none of them is captured yet (§ Schema).

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

Per **result**: status, reason, expected/actual and predicate syntax (from the
`assert` macro, [test-runner](test-runner.md)), and the emitted signal on
failure. A result's `tier` names where it ran ([test-runner](test-runner.md)).
A result that a form or a child produced also records what it cost. For a form
in a worker, `wall_ms` runs from handing the form over to having its answer,
and `cpu_us` is the delta of `(clock/cpu)` on the thread that ran it. That
thread's CPU leaves out the JIT's compile thread, the I/O pool and any child
the form starts. For an `--isolate` child, `wall_ms` runs from spawn to reap,
and `cpu_us` (user plus system) and `max_rss_kb` are its total from
`subprocess/rusage` ([subprocess](subprocess.md)). A worker that never hands
back its answer — a missed deadline, a panic — leaves `cpu_us` NULL. A
file-level error or skip and a divergence row leave all three NULL, because
nothing ran to produce them.

> CPU delta, not fuel. Fuel (`SIG_FUEL`) is specific to the `std/process`
> scheduler, is not consumed by Elle's default root scheduler, and essentially
> counts continuation-passing rather than CPU work — so it does not capture
> cost. A `(clock/cpu)` before/after delta does. It is not bit-for-bit
> deterministic, so regression queries on it compare distributions/thresholds,
> not exact equality; for that, prefer many runs (which we keep) over one.

Per **asset**: the runner captures each result's stdout and stderr into the
filesystem CAS ([test-runner](test-runner.md)), deduped by hash, so identical
output across runs costs one file. History is **kept indefinitely**: nothing
prunes it, and `elle test --reset` removes the whole store.

> The design also captures the full `--dump` artifact set (`ast, fhir, defuse,
> regions, escape, hir, lir, cfg, dfa, jit`) and `--dump=stats` per result, and
> `--trace` for failing forms only. None of them is captured
> ([test-runner](test-runner.md) § CAS asset capture): the `--dump` pass OOMs
> the corpus run and does not dedup, and it returns once the region leak it
> exposes is fixed.

## The boot fingerprint

A verdict is a function of the form, the binary, the boot sources, and the run
configuration. A `run` row named the commit and the machine and said nothing
about the executable, so no result could be safely reused, and archaeology
across compiler changes had no axis to group by.

`boot_fingerprint` is that axis: one hash over the bytes of the running
executable. The three sources a boot compiles to become a runtime —
[core.lisp](../src/core.lisp), [prelude.lisp](../src/prelude.lisp) and
[stdlib.lisp](../src/stdlib.lisp) — are built into that executable, so one
hash covers the binary and the boot sources together. Edit any of them, or
change the compiler, and the rebuilt binary hashes differently; every result
recorded under the old value belongs to the build that produced it.

Only the binary can report this, so it arrives as `(elle/boot-fingerprint)`,
beside the version and the profile ([test-cli](test-cli.md)). On a
box whose OS will not name the running executable the column is NULL: the run
happened, and nothing identified what ran it.

The value is the hash itself, a 64-bit number, and the column holds it as one.
Nothing displays a fingerprint; what reads it compares, groups and joins it,
and the same hash rendered as text costs twice the bytes and compares a
character at a time. The form hash and the CAS address are text because each
also names a file; a fingerprint names nothing.

[image](impl/image.md) computes an identity of its own to gate hydration.
Converge on one implementation when that lands.

## What analysis says about a form

A `form` row carries three columns the compiler fills: `signal`, `caps` and
`touches`. They are what makes a category a query rather than a directory
(§ The durable corpus is a flat set), and a form's effect profile is also what
decides whether its result can be replayed.

The runner analyzes each file once, at scan time, before it runs anything. It
wraps the file's source in one function and reads that function's inferred
profile back through `compile/analyze`. A file's top level and a function body
are both letrec-scoped, so the wrapped body is the form, and the function's
inferred signal is the form's own effect profile.

| Column | What it holds |
|--------|---------------|
| `signal` | every signal bit the form may emit, named and space separated: `error fs io` |
| `caps` | the capability bits among them: `debug`, `exec`, `ffi`, `fs`, `gpu`, `io`, `os-signal` |
| `touches` | every binding the form's analysis calls, less the ones it defines itself: `= port/open struct` |

`error` and `yield` reach `signal` and never `caps`. A form that raises, and a
form that suspends, are both still functions of their own inputs; a form that
opens a file is not.

**An empty column is a claim, and NULL is not.** An empty `caps` says the
compiler proved the form reaches no capability, which is what makes its result
replayable under one boot fingerprint (§ The boot fingerprint). NULL says the
analysis did not answer, and nothing may be read into it. A call the compiler
cannot resolve answers with every capability bit, so a form that calls a stdlib
closure records the whole set — the honest over-approximation, and the
direction a skip decision has to round toward.

The unit is the file because the row is the file: a multi-form file is one
whole-file form, and the durable corpus is one form per file, so a file's
profile is its form's profile.

## Measurements: a reading a query can read

A producer measures a shape and prints one `measure` line per reading, with
the subject, the axis, the value, its interval's half-width and its unit
([ratchet](ratchet.md)). Printed, a reading is prose: nothing can ask what it
was three commits ago, and nothing can check the producer's coverage against
a declared set. So the runner reads the lines back and records them.

Stdout is the channel. The runner captures stdout for every form on every
tier and for every isolated child, and it reads every `measure` line out of
each capture. So a producer in a worker thread and a dashboard in its own
process land the same way. Nothing is set in an environment, and a direct
`elle-rig tests/impl/oracle.lisp` run prints the same lines and records
nothing.

A run with no build writes no `measurement` row: there are no rows to judge a
reading against, so it is neither recorded nor judged. A run with a build
judges each reading against the row that build holds in the ledger of the file
that printed it, with the judge of [the ledger module](../lib/ratchet/ledger.lisp).
It is written as one `measurement` row carrying the row's bound and kind and
the verdict:

| Verdict | Meaning |
|---------|---------|
| `ok` | within its bound |
| `regression` | past the bound the worse way |
| `stale` | past the bound the better way; `--repin` moves the bound |
| `unledgered` | the producer has a ledger and this reading has no row in it |
| `missing` | a row of the producer's ledger that this result printed no reading for |
| `void` | the instrument refused the reading: a dead gauge, or a rate the block size moved |
| NULL | the producer has no ledger, or none of its rows belongs to the run's build, so the reading is recorded and not judged |

The gate fails on any verdict but `ok` and NULL. A NULL says the producer is
not ratcheted on this build yet, which is how a build keeps its history in the
table before its rows exist. Once the ledger holds a row of the build, every
reading the producer prints there must have a row, and every row of that
build must get a reading.

A `missing` row is written after a result lands as `pass`, one per row of the
run's build that result printed no reading for, against that result. A result that
failed already says so, and a gated one skipped rather than fell silent, so
neither is asked for its rows.

A run that recorded any reading says so, tallied by verdict, and names every
reading that is neither `ok` nor unjudged:

```
412 readings · 409 ok · 1 regression · 1 stale · 1 missing
  regression  tests/impl/oracle.lisp  [process]  reduce  objects  1.31 ±0.12 objects/op  pinned 1.002
  stale  tests/impl/plumb.lisp  [process]  ev-abort  regions  0.0 ±0.03 regions/op  pinned 1
  missing  tests/impl/oracle.lisp  [process]  fiber-nested  regions
```

The rest is a query. The summary is a reading aid, and every number in it comes
out of the table.

## The heap gauges

The runner records what each file cost its own heap and the heaps its test code
ran on, one `gauge` row per file, gauge and heap. [test-gauges](test-gauges.md)
owns the gauges, the two heaps, and the summary block that names the top
growers.

## Schema

```sql
CREATE TABLE run (                  -- one row per `elle test` invocation
  id INTEGER PRIMARY KEY, started_at TEXT,
  run_key TEXT,                     -- the run's identity across stores; UNIQUE, so an import repeats safely
  finished_at TEXT,                 -- stamped at completion; NULL = killed, or still running
  git_commit TEXT, git_dirty INT, tree_hash TEXT, worktree TEXT,  -- the code state this run ran against
  boot_fingerprint INT,             -- the binary and the boot sources, hashed
  elle_version TEXT, build_profile TEXT, host TEXT, argv TEXT,
  build TEXT,                       -- the build's key, tier-backend-os-arch, or NULL (ratchet.md)
  tiers TEXT,                       -- the probed tiers (vm,jit,…), or process
  pid INT,                          -- the runner's process on `host`; tells a live run from a killed one
  selection TEXT,                   -- the filter predicate; NULL = full run (the gate)
  n_selected INT,                   -- files + -e forms planned; written at insert
  n_pass INT, n_fail INT, n_skip INT, n_timeout INT,  -- aggregated at completion only
  n_diverge INT,                    -- forms whose tiers disagreed; aggregated at completion
  wall_ms INT, max_rss_kb INT, cpu_user_ms INT, cpu_sys_ms INT);   -- resource usage; not created yet

CREATE TABLE changed_file (         -- working tree vs HEAD at run time
  run_id INT REFERENCES run(id), path TEXT, status TEXT, blob_hash TEXT);

CREATE TABLE form (                 -- deduped across runs; the computer names it
  hash TEXT PRIMARY KEY,            -- hash of the read Syntax (comments elided)
  origin TEXT,                      -- a .lisp path (durable, in git), or ':adhoc'
  session TEXT,                     -- session id for ad-hoc forms; NULL for durable
  file TEXT, form_index INT, line INT, col INT,
  label TEXT,                       -- derived: assert message / leading symbols
  src TEXT,                         -- the form's syntax, rendered for display
  caps TEXT, touches TEXT, signal TEXT);   -- from compile/analyze (§ What analysis says about a form)

CREATE TABLE result (               -- one row per (form × tier × run)
  id INTEGER PRIMARY KEY, run_id INT REFERENCES run(id),
  form_hash TEXT REFERENCES form(hash),
  tier TEXT,                        -- vm|jit|wasm|mlir-cpu|process, or * for a divergence
  status TEXT,                      -- pass|fail|skip|timeout|diverge
  reason TEXT, expected TEXT, actual TEXT, syntax TEXT, signal TEXT,
  wall_ms INT, cpu_us INT,          -- what it cost (§ What gets captured)
  max_rss_kb INT);                  -- an --isolate child's peak resident set

CREATE TABLE asset (                -- artifact attached to a result; bytes live in the CAS
  result_id INT REFERENCES result(id),
  kind TEXT,                        -- ast|fhir|hir|lir|cfg|dfa|jit|stats|stdout|stderr|trace
  hash TEXT, size INT, codec TEXT); -- bytes at <db-dir>/cas/<hash>; codec e.g. zstd

CREATE TABLE measurement (          -- one reading, judged against its ledger row
  run_id INT REFERENCES run(id),
  result_id INT REFERENCES result(id),  -- the form × tier, or the child, that printed it
  subject TEXT, axis TEXT,          -- the shape, and the dimension it was read on
  value REAL, half REAL, unit TEXT, -- the reading, its interval's half-width, what one unit is
  bound REAL, kind TEXT,            -- the row it met: pin|floor|ceiling; NULL when it met none
  verdict TEXT);                    -- ok|regression|stale|unledgered|missing|void; NULL = not judged

CREATE TABLE gauge (                -- what one file cost one heap, on one gauge
  id INTEGER PRIMARY KEY,           -- insertion order, which is boundary order
  run_id INT REFERENCES run(id),
  file TEXT,                        -- the file the change is charged to
  heap TEXT,                        -- runner|test; NULL in an older store = runner
  kind TEXT,                        -- the gauge: objects|regions|pages|adopts|… (test-gauges.md)
  delta INT,                        -- runner: the change since the previous boundary; test: the sum over the file's runs
  reading INT);                     -- runner: the gauge at this boundary; test: NULL
```

The runner writes this with `lib/sqlite.lisp` (FFI to libsqlite3). The DB holds
only metadata and hashes; artifact bytes live in the on-disk CAS, so the file
stays small and merge/diff concerns never arise (it is gitignored regardless).

**What the runner creates ([store.lisp](../src/test/store.lisp) `ensure-schema`).**
The runner creates `result`, `asset`, `measurement` and `gauge` with the
columns above, and a session DB written before `result.max_rss_kb` existed
gains it by `ALTER TABLE`. `run`, `form` and `changed_file` are subsets:

- `run` carries every column above except the resource ones
  (`wall_ms`/`max_rss_kb`/`cpu_user_ms`/`cpu_sys_ms`), which are deferred. So a
  resource query is design-only until they land; a `SELECT` of a deferred
  column errors with `no such column`. A session DB written before the
  code-state, fingerprint, key, pid or build columns existed gains them by
  `ALTER TABLE`, with NULL for every run recorded until then. `gauge` gains
  `heap` the same way.
- `form` is written without `line`, `col` and `session`: a form's location and
  an ad-hoc form's session id are deferred, and each reads NULL. The three
  analysis columns are written at scan time (§ What analysis says about a
  form).
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
SELECT run.git_commit AS sha, m.value AS rate, m.half AS half, m.bound AS pinned,
       m.verdict AS verdict
FROM measurement m JOIN run ON run.id = m.run_id
WHERE m.subject = 'io-drop' AND m.axis = 'regions' ORDER BY m.run_id;

-- The forms a cache may replay: analyzed, and proved to reach no capability.
SELECT file, label FROM form WHERE caps = '' ORDER BY file;

-- Every form whose analysis calls one binding — category as a query.
SELECT file FROM form WHERE instr(' ' || touches || ' ', ' chan/send ') > 0;

-- One form's history, grouped by the build that produced each verdict.
SELECT run.boot_fingerprint AS build, result.status AS status, count(*) AS n
FROM result JOIN run ON run.id = result.run_id
WHERE result.form_hash = ? GROUP BY build, status;
```

