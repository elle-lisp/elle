# One test system

<!-- audited: 2026-09-30 -->

The plan that folds every test product into `elle test`, keeps the results,
and states what a run may skip.

## Where we are

Five products test this repository today:

- `elle test` runs both Elle suites and records every result in a SQLite
  session DB ([test-store](test-store.md)). The gate targets run each
  implementation file as its own child, `elle-rig FILE` ([testing](testing.md)).
- [oracle.lisp](../tests/impl/oracle.lisp) and
  [plumb.lisp](../tests/impl/plumb.lisp) measure leak rates. They are
  implementation tests, so they run on the rig with the rest of the
  implementation suite. Each reading they print lands in the session DB
  ([test-store](test-store.md)), so a rate's history is a query.
- [escape-golden.lisp](../tests/impl/escape-golden.lisp) pins escape snapshots.
- The Rust suite runs under `cargo test`, outside the session DB.
- CI builds several implementations, and each runs the language suite
  ([ci](analysis/ci.md)).

The runner's thesis is "capture everything once; query forever", and the data
now survives the run that produced it. The DB lives in the state directory, so
a reboot keeps it, and each run names the commit, worktree, host, and build it
ran against ([test-store](test-store.md)). A CI job publishes its store as an
artifact ([ci](analysis/ci.md)) and `--import` merges a downloaded one into
local history, so a failure on a box you cannot reach is a query rather than a
log.

## The decisions

### Results are state

The session DB and CAS live in a persistent state directory. `ELLE_CACHE`
keeps the things a rebuild can regenerate; run history is a record, so it
lives with state.

Every CI job that records runs uploads its DB and CAS as an artifact
([ci](analysis/ci.md)), and `elle test --import` appends a downloaded run to
local history ([test-store](test-store.md)). The schema makes the merge cheap —
forms are keyed by syntax hash, assets by content hash, and runs append under a
key that makes a repeat import a no-op. A run row already carries the commit,
worktree, host, and build, so an imported run says what it ran against.

Later, the store becomes shared: a content-addressed blob store plus a small
index, Redis first, exactly the `store` milestone in [fleet](impl/fleet.md).
Dev boxes, CI, and fleet workers then append to one history.

### One scheduler

`elle test` schedules everything. The oracle, plumb, the guardfree family, and
every suite pass are runs it owns, and their verdicts land in the same DB.
The Makefile keeps `make smoke` as the entry point, and it names each suite's
files and each rig profile rather than a matrix of passes.

### Builds and the rig replace the pass matrix

A language test runs on the runtime a build ships, with no flag
([spec](spec.md) § Two suites). So the language suite has no pass matrix: its
matrix is the set of builds CI makes, and each build is one implementation.

An implementation test that needs a mode names it in a sidecar beside the
file, and the rig reads it ([rig](../rig/overview.md)). A profile is the same
settings applied to every file of a pass. The implementation suite runs the
language suite under two: every function compiled on its first call, and on
macOS each released page scrubbed. A sidecar lives outside the source, so the
file keeps its meaning under a plain `elle-rig FILE`.

A completeness gate remains to build: it fails when a declared profile records
no verdicts, the same shape as the oracle's `@dual-read` table.

### Budgets come from history

The Makefile already tells people to "read the budget from a timed run, never
from a number written here". The runner has the timed runs, so it applies the
rule itself: a form's budget is a multiple of its own recorded wall time, with
a floor at the default for new forms. `WIDE_FAMILIES`, `WIDE_TIMEOUT_MS`,
`DOCTEST_TIMEOUT`, and `PLUGIN_TIMEOUT` are deleted. A file that asserts its own deadline keeps doing
so in ordinary code; the harness budget is the backstop.

### The suites stay plain Elle

The harness consumes values and signals, and never reads syntax. A test that
needs a missing dependency raises an error with kind `:gated` and a reason;
the runner records a reasoned skip. `gate!` may exist as a prelude macro,
because it means the same thing under a plain `elle FILE` run — refuse with a
reason — and it must expand to the same value convention, so code that spells
it out by hand is served equally.

The test for any proposed in-file form: it must have semantics under plain
`elle`, with no harness present. A budget or mode declaration fails that test,
so budgets are measured (above) and modes live in a sidecar (above).

### Only the fingerprint grants a skip

A verdict is a function of the form, the binary, the boot sources, and the
profile. So a cached result is reusable only under a total key: the form's
closure hash times the boot fingerprint times the profile. The boot
fingerprint covers the binary and everything it reads to become a runtime —
the same identity [image](impl/image.md) computes to gate hydration and
[fleet](impl/fleet.md) uses for routing. Build it once; three systems consume
it.

The fingerprint is in: every run records one ([test-store](test-store.md)).
What is missing is the lookup that spends it.

Under this key, a compiler change moves the fingerprint and every cached
result misses, so both suites re-run. A change to one suite file misses only
its closure. `--changed` becomes a cache lookup with no impact heuristic
inside it.

The rule for every derived signal: **derived impact may reorder work; only
fingerprint identity may skip it.** Artifact diffs, `--impacted-by`, and
failure recency order the run so the likely failures land first. The gate
remains a full run.

Within one fingerprint, a skip also needs the environment to hold still. The
compiler's capability inference already classifies a form's effects, so the
runner caches pure-compute forms and re-runs forms that touch io, net, ffi, or
the clock. Each form's profile is recorded as it is scanned
([test-store](test-store.md)), so the classification is a column rather than a
re-analysis.

### Forms, where the compiler can prove it

The durable unit is one form. A legacy multi-form file slices into per-form
results when whole-module analysis proves the expression forms independent:
each form reads only bindings that no form mutates, and no port or process
state threads between them. A sequential scenario stays one form. Slicing buys
per-form failure sets, per-form cache keys, and finer slices for expensive
profiles.

### Measurements join results

A producer prints one line per reading, and the runner records it into a
`measurement` table: subject, axis, value, half-width, unit, and the verdict
the ledger row gave it ([test-store](test-store.md)). Rate history across
commits is a query. The coverage question in elle-lisp/elle#1144 is the
ledger: a committed table of (subject, axis) rows per producer, judged once
in the runner for every producer, where a row nobody read is `missing`
([ratchet](ratchet.md)).

The runner's own growth is in, on a table of its own: it samples the arena
gauges between files and charges each file what it cost
([test-store](test-store.md)). That is the evidence needed to retire
`CORPUS_BATCH` and, once the `compile/dumps` leak closes, to restore dump
capture.

## What this deletes

Every hand-set timeout variable; `CORPUS_BATCH` and the batch dealing; and the
CI habit of reading failures out of logs.

## Landing order

1. Persistence: in. The state directory, the `run` identity columns, the CI
   artifact upload and `--import`.
2. Builds and the rig: in. The language suite runs on every build with no
   flag; the guardfree family, the oracle and plumb run on the rig; the pass
   matrix and `elle_scripts.rs` are gone.
3. Derived budgets.
4. The coverage gate (elle-lisp/elle#1144), then the runtime-structure gauges
   (elle-lisp/elle#1143, elle-lisp/elle#1135). The reading line and the
   ledger's `missing` gate are in; the dashboards move onto the ledger next.
5. Content-keyed results; ordering signals. The boot fingerprint and the
   per-form effect profile are in.
6. Provable form slicing; parity rows (elle-lisp/elle#1142); golden
   comparisons that store both sides (elle-lisp/elle#1138).
