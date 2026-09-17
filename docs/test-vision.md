# One test system

<!-- audited: 2026-09-17 -->

The plan that folds every test product into `elle test`, keeps the results,
and states what a run may skip.

## Where we are

Six products test this repository today:

- `elle test` runs the corpus and records every result in a SQLite session DB
  ([test-runner](test-runner.md)).
- [oracle.lisp](../tests/elle/oracle.lisp) and
  [plumb.lisp](../tests/elle/plumb.lisp) measure leak rates. The Makefile runs
  them outside the corpus, under hand-set timeouts.
- [escape-golden.lisp](../tests/elle/escape-golden.lisp) pins escape snapshots.
- `tests/integration/elle_scripts.rs` runs the files that need a process-global
  flag (`--trace=guardfree`, `--no-uring`), each as a cargo-driven subprocess.
- The Makefile runs five more corpus passes: per-file vm, per-file jit,
  nouring, mlir, wasm — each with its own skip and timeout lists.

The runner's thesis is "capture everything once; query forever", and the data
does not survive:

- CI writes the session DB inside the runner and uploads nothing. When a job
  fails, the reader gets the log, which is the medium the runner was built to
  replace.
- Locally the DB lives under `ELLE_CACHE`, which sits on tmpfs. A reboot
  erases the whole history.

## The decisions

### Results are state

The session DB and CAS move to a persistent state directory. `ELLE_CACHE`
keeps the things a rebuild can regenerate; run history is a record, so it
lives with state.

Every CI corpus job uploads its DB and CAS as an artifact. A new
`elle test --import` merges a downloaded run into local history. The schema
makes the merge cheap: forms are keyed by syntax hash, assets by content hash,
and runs append. The prerequisite is the deferred `run` columns — commit,
host, and config — so an imported run says what it ran against.

Later, the store becomes shared: a content-addressed blob store plus a small
index, Redis first, exactly the `store` milestone in [fleet](impl/fleet.md).
Dev boxes, CI, and fleet workers then append to one history.

### One scheduler

`elle test` schedules everything. The oracle, plumb, the guardfree family, and
the per-file passes become runs it owns, and their verdicts land in the same
DB. The Makefile keeps `make smoke` as the entry point and loses the pass
matrix, the skip lists, and the timeout variables.

### Profiles replace the pass matrix

A profile is data: a name, a flag set, an isolation choice, and a selection
query. Profiles live in one file in the repository, and each run records the
profile it ran under.

Isolation is the piece that unlocks the rest. A profile can run its selection
in child processes, so a guardfree SIGSEGV kills one child and lands as a
recorded failure. That absorbs the `elle_scripts.rs` family, and it gives the
per-file teardown passes (`smoke-vm`, `smoke-jit`, `smoke-nouring`) a home as
profiles named process, jit, and pool.

Profiles add coverage; the default profile still runs everything. A
completeness gate fails when a declared profile records no verdicts, the same
shape as the oracle's `@dual-read` table.

### Budgets come from history

The Makefile already tells people to "read the budget from a timed run, never
from a number written here". The runner has the timed runs, so it applies the
rule itself: a form's budget is a multiple of its own recorded wall time, with
a floor at the default for new forms. `WIDE_FILES`, `ORACLE_TIMEOUT`,
`DOCTEST_TIMEOUT`, and `PLUGIN_TIMEOUT` are deleted. A file that asserts its
own deadline keeps doing so in ordinary code; the harness budget is the
backstop.

### The corpus stays plain Elle

The harness consumes values and signals, and never reads syntax. A test that
needs a missing dependency raises an error with kind `:gated` and a reason;
the runner records a reasoned skip. `gate!` may exist as a prelude macro,
because it means the same thing under a plain `elle FILE` run — refuse with a
reason — and it must expand to the same value convention, so code that spells
it out by hand is served equally.

The test for any proposed in-file form: it must have semantics under plain
`elle`, with no harness present. A budget or mode declaration fails that test,
so budgets are measured (above) and modes are profiles (above).

### Only the fingerprint grants a skip

A verdict is a function of the form, the binary, the boot sources, and the
profile. So a cached result is reusable only under a total key: the form's
closure hash times the boot fingerprint times the profile. The boot
fingerprint covers the binary and everything it reads to become a runtime —
the same identity [image](impl/image.md) computes to gate hydration and
[fleet](impl/fleet.md) uses for routing. Build it once; three systems consume
it.

Under this key, a compiler change moves the fingerprint and every cached
result misses, so the full corpus re-runs. A change to one corpus file misses
only its closure. `--changed` becomes a cache lookup with no impact heuristic
inside it.

The rule for every derived signal: **derived impact may reorder work; only
fingerprint identity may skip it.** Artifact diffs, `--impacted-by`, and
failure recency order the run so the likely failures land first. The gate
remains a full run.

Within one fingerprint, a skip also needs the environment to hold still. The
compiler's capability inference already classifies a form's effects, so the
runner caches pure-compute forms and re-runs forms that touch io, net, ffi, or
the clock.

### Forms, where the compiler can prove it

The durable unit is one form. A legacy multi-form file slices into per-form
results when whole-module analysis proves the expression forms independent:
each form reads only bindings that no form mutates, and no port or process
state threads between them. A sequential scenario stays one form. Slicing buys
per-form failure sets, per-form divergence, per-form cache keys, and finer
slices for expensive profiles.

### Measurements join results

The dashboards keep their instruments — the estimator, the discriminators, the
by-design set — and report each verdict through a structured channel the
runner records into a `measurement` table: subject, axis, value, verdict. The
coverage question in elle-lisp/elle#1144 then becomes a gated table of
(subject, axis) rows, checked once in the runner for every dashboard. Rate
history across commits becomes a query.

The runner also samples the arena gauges between files and records the deltas.
That turns its own growth into attributed measurements, which is the evidence
needed to retire `CORPUS_BATCH` and, once the `compile/dumps` leak closes, to
restore dump capture.

## What this deletes

The Makefile pass matrix and its skip lists; every hand-set timeout variable;
`CORPUS_BATCH` and the batch dealing; the `elle_scripts.rs` pin harness; and
the CI habit of reading failures out of logs.

## Landing order

1. Persistence: the state directory, the CI artifact upload, `--import`, and
   the `run` identity columns.
2. Profiles with child-process isolation; fold in the guardfree family, the
   oracle, plumb, and the per-file passes.
3. Derived budgets.
4. The measurement channel and coverage gate (elle-lisp/elle#1144), then the
   runtime-structure gauges (elle-lisp/elle#1143, elle-lisp/elle#1135).
5. The boot fingerprint and content-keyed results; ordering signals.
6. Provable form slicing; parity rows (elle-lisp/elle#1142); golden
   comparisons that store both sides (elle-lisp/elle#1138).
