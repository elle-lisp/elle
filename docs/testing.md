# Testing

<!-- audited: 2026-10-23 -->

The two test suites, what each one claims, the builds that run them, and how a
run is read.

Elle has a language suite and an implementation suite
([spec](spec.md)):

1. **The language suite** — `.lisp` files under
   [tests/lang](../tests/lang/overview.md). Each asserts what the language
   promises, and every build runs every file with no flag.
2. **The implementation suite** — `.lisp` files under
   [tests/impl](../tests/impl/overview.md), each run on
   [the rig](../rig/overview.md), and the Rust suite under `tests/` and in
   inline `#[cfg(test)]` modules, run through `cargo test`. See
   [tests/AGENTS.md](../tests/AGENTS.md) for the Rust categories and helpers.

[docs/analysis/testing.md](analysis/testing.md) is the decision tree: given a
thing to test, which suite, and which kind of test. This document covers the
two Elle directories and the runner that drives them. The runner's full
specification is [docs/test-runner.md](test-runner.md), with
[docs/test-cli.md](test-cli.md) for its command line and
[docs/test-store.md](test-store.md) for what it records.

## Quick start

| Command | What it does |
|---------|--------------|
| `make smoke-lang` | The language suite |
| `make smoke-impl` | The implementation suite on the rig |
| `make smoke-eager` | Both suites on the rig, every function compiled on its first call |
| `make smoke-producers` | The files a ledger names, less the three `make tree-producers` runs, in-process on the rig |
| `make smoke-charge` | Each file's charge on the runner's heap |
| `make tree-producers` | The audit, index and size ledgers on the release rig; `make qa` runs it |
| `make qa` | The `QA` job ([ci](analysis/ci.md)) and the Android cross-check: about two minutes, and two more when the release rig needs a build |
| `make smoke` | `make qa`, then the five `smoke-` passes above, the doctests, the embedding demo, and the surface gate |
| `make test` | `make smoke`, then the Rust unit and integration tests |
| `elle test tests/lang/*.lisp` | Run those files in-process; print a summary; gate on exit code |
| `elle-rig tests/impl/NAME.lisp` | Run one implementation test with its sidecar |
| `elle test --summary` | Re-print the last run's summary (no re-run) |
| `elle test --query 'SQL'` | Run ad-hoc SQL |

A run prints its run line, its tally, and a line per problem, all to stderr:

```
elle test · run 7 of 7 · commit a1b2c3d (dirty) · 41.2 s
184 pass · 6 skip · 1 fail · 0 diverge · 1 timeout
2 problems (query the DB for full detail):
  fail     tests/lang/foo.lisp:12  [process]  expected 42, got 41
  timeout  tests/lang/subprocess.lisp  [process]  child exceeded the 60000 ms budget
```

The run line names the code the tally describes, and ends with the run's wall
time. A run outside a repository names no commit. A run under `elle-rig test`
names its build before the time, `· build jit-uring-linux-x86_64`, the build
its readings were judged as ([docs/ratchet.md](ratchet.md)).

A run that passed prints its run line and its tally, and nothing else. A
reading that gates adds its lines ([docs/ratchet.md](ratchet.md)). What each
file cost each heap, and every reading that passed, stay in the store: a suite
pass prints one summary per batch ([ci](analysis/ci.md)), and lines that report
nothing wrong, repeated per batch, bury the lines that do.

You read results from the run itself — never by hand-writing SQLite.

## A build is an implementation

Every build CI makes runs the language suite, and a file that passes on one
build and fails on another has found a defect in the build that fails.
[ci](analysis/ci.md) names the builds and the passes each one runs.

## The agent-first runner (`elle test`)

The runner compiles each file as a real program, runs it in a worker thread on
every tier its build carries, and records every result in a SQLite store in
the state directory ([docs/test-runner.md](test-runner.md),
[docs/test-store.md](test-store.md)). An agent then queries the stored run
instead of re-running it ([docs/test-cli.md](test-cli.md)).

`--isolate 'FLAGS'` runs each path as its own child, recorded on the `process`
tier, which is how the implementation suite runs on the rig. A file that
measures prints one `measure` line per reading, and under `elle-rig test` each
reading is judged against the file's ledger ([docs/ratchet.md](ratchet.md)).

```sh
elle test --isolate '' tests/lang/closures.lisp
target/release/elle-rig test --isolate '' tests/impl/oracle.lisp
```

### Statuses

| Status | Meaning | Gates? |
|--------|---------|--------|
| `pass` | the file ran to its end | no |
| `skip` | gated out (`gate!`/`:gated`), tier-ineligible, or `(exit 0)` | no |
| `fail` | an assertion or error | **yes** |
| `diverge` | tiers returned different values (synthetic `tier='*'` row) | **yes** |
| `timeout` | the file exceeded its budget | **yes** |

The gate (exit code) is zero iff no form failed, diverged, or timed out. `status`
and `tier` are keyword-valued in the runner and stored as their bare name in the
TEXT columns (`WHERE status = 'pass'` works as written).

## Adding an Elle test

Decide the suite first ([docs/analysis/testing.md](analysis/testing.md)). A
claim every correct implementation meets goes in `tests/lang/`; a claim about
this implementation goes in `tests/impl/`. Either way, write a `.lisp` file
using the one idiom — `(assert COND "message")`. There is no `deftest`, no
suite DSL. The runner scavenges the form's first `assert` message as the test's
label.

```lisp
(elle/epoch 13)
## what this file checks
(assert (= (+ 1 1) 2) "addition works")
```

An implementation test that needs a mode names it in a sidecar beside it
([rig](../rig/overview.md)).

### Gating, not skip-lists

A test that needs an optional dependency (an FFI library, a GPU, a running
service) **gates itself in-file** — there are no Makefile skip lists for the
runner. Re-raise a missing dependency as `:gated` so the runner records a
reasoned `skip` (and a direct `elle FILE` run exits 0 cleanly):

```lisp
(def [ok? plugin] (protect (import "plugin/myplugin")))
(unless ok?
  (error (struct :error :gated :reason "myplugin plugin not built")))
```

Name a plugin through `import`, not through an `import-file` path. `import`
resolves `plugin/X` against the running binary's own build profile
([modules.md](modules.md)); a written-out
`target/release/…` names a file only a release build has, so under a debug
binary that test gates itself out and reports nothing.

A language test never gates on a tier: every build runs it, and it must pass on
every one. **Never** `(exit 0)` to skip — under the runner the trap turns it
into a `skip`, but `:gated` carries a *reason* and is the intended idiom.

### Naming resources outside the process

A test that names anything another process can see — a Redis key, a temp file, a
socket path — must carry the running process's pid in the name. Two runs of one
file share every fixed name: a second checkout of this project, a rerun that
overlaps the first, or the same file launched twice. Each run then writes and
deletes the other's state mid-flight, and a reader sees its own value missing.
That failure reads exactly like the defect the test exists to catch, so the test
can no longer tell you which one happened.

Build every name through one helper, and match the cleanup pattern against the
same prefix:

```lisp
(def key-prefix (string "test:redis:" (sys/pid) ":"))
(defn test-key [name] (string key-prefix name))
```

A per-file namespace does not cover this. It separates different files, not two
runs of one file.

### A performance gate measures against a control

A performance gate is an implementation test: the language promises a result,
not its cost. A test that pins a *cost* — a bulk copy against a per-byte copy, a
linear pass against a quadratic one — cannot assert a wall-clock number. The
runner shares its machine, so a bound wide enough to survive a stall is wider
than the regression it exists to catch.
[bytes-linear.lisp](../tests/impl/bytes-linear.lisp) failed at 0.5055s against
a 0.5 bound, on a commit that costs 0.003s on a quiet box.

Measure a **control** instead. Pick an operation of the same size, in the same
process, that runs the path the regression cannot reach, and require the
subject to stay within a small multiple of it. A slow machine slows both
measurements, so the ratio survives what an absolute bound does not, and a
per-byte path costs about a hundred times the bulk one.

Three rules keep the ratio steady:

- **Time the thread's CPU, not the wall.** A child on a crowded runner can
  wait for a CPU through every round of one thunk and through none of the
  other's. `clock/cpu` counts only the time the calling thread ran, so the
  wait never enters a timing. The thunk must do its work on that thread.
- **Alternate the two measurements, and keep the smallest of several rounds.**
  Both thunks then sample the same stretch of machine, and one slow round
  decides nothing.
- **Build the operands outside the timed thunk.** `length` on a string counts
  graphemes, so a loop that re-reads it times the walk instead of the work.

`best-of` in [the ratchet](../lib/ratchet.md) keeps the first two rules, so
time a gate with it rather than with a loop of your own. The worked examples
are [bytes-linear.lisp](../tests/impl/bytes-linear.lisp), which gates a binary
append against the same-size text append,
[concat-linear.lisp](../tests/impl/concat-linear.lisp), which gates a string
concat against the same-size bytes concat, and
[apply-tail-linear.lisp](../tests/impl/apply-tail-linear.lisp), which gates a
tail call against the same call in non-tail position.

A **timeout** test is the other case, and it keeps its absolute bound. There
the deadline is the specification: a 50 ms `chan/select` has to return in about
50 ms, and no ratio can say what that means.

## Reading a run

```sh
elle test --summary                       # latest run's tally + problems
elle test --query 'SELECT * FROM run'     # run history
elle test --query \
  "SELECT f.file, r.tier, r.reason FROM result r
   JOIN form f ON f.hash = r.form_hash WHERE r.status = 'fail'"
```

[docs/test-store.md](test-store.md) holds the schema. Each `run` row names the
code it ran against and the machine that ran it, and each result's stdout and
stderr sit in the CAS beside the database. What each file cost each heap is a
`gauge` row on every gauge ([docs/test-gauges.md](test-gauges.md)):

```sh
elle test --query \
  "SELECT file, sum(delta) AS regions FROM gauge
   WHERE kind = 'regions' AND heap = 'runner'
   GROUP BY file ORDER BY regions DESC LIMIT 10"
```

A form that misses its deadline prints a native backtrace of every thread in
the runner to stderr, under `── threads at the deadline ──`. The runner abandons
a timed-out worker rather than killing it, so the wedged thread is still parked
where it stopped. That stack is the only account of a hang that happens on one
machine alone. The sampler is `sample` on macOS and `eu-stack` on Linux; a box
with neither prints nothing.

A run that never finished reads as killed or as still running, never as green
([docs/test-runner.md](test-runner.md)).

## Correctness the leak and UAF oracles cannot see

The two automated memory oracles each have a blind spot.
[oracle.lisp](../tests/impl/oracle.lisp) measures heap *growth* — in objects,
regions, bytes, or physical region ids — so it sees a leak, never a wrong
answer. `--trace=guardfree` faults on a *use-after-free* — it sees a dangling
read, never a live read of the wrong live value. A computation that returns a
**wrong-but-well-typed value** slips past both: no region leaked, no freed page
was touched, the result is just silently incorrect.

Self-recursion across a control-flow boundary is exactly that kind of hazard. A
self-recursive local function must recurse to *itself* — the same body, with its own
captured environment — no matter what boundary the recursion crosses:

- a **yield/resume** (the activation is parked and replayed),
- a **tail-call frame replacement** (the activation is reused in place), or
- being **handed off as a value** (passed to a higher-order call, returned, or stored,
  then invoked).

The runtime carries the executing function's identity across each of these. If that
identity goes stale, the recursion silently continues as a *different* closure (or with
a *different* captured environment) and returns a plausible wrong value — invisible to
both oracles above. So this correctness is pinned **behaviorally**, by value
assertions, not by a memory gauge: the
[recur-after-yield.lisp](../tests/lang/recur-after-yield.lisp),
[recur-after-tail-call.lisp](../tests/lang/recur-after-tail-call.lisp), and
[recur-as-value.lisp](../tests/lang/recur-as-value.lisp) language tests (run on
every build), with deterministic peers in
[selfrec.rs](../src/runtime/tests/selfrec.rs). Each asserts a result that is
only correct if the self-identity survived the boundary, so a stale
self-reference flips the assertion red. They are the regression guard for any
change to how a self-reference is resolved or how an activation is carried
across yield, tail call, or value handoff.

The **order of two correctly-counted releases** is the other hazard of this kind, and
it needs a third detector rather than a behavioral pin. A captured binding's value and
its env cell are two regions addressed by one env index; the value's release loads the
box raw and unwraps it, so it reads the page the box's release frees
([docs/impl/region/cells.md](impl/region/cells.md)). Emit the two in the wrong order and
both counts are still right: nothing leaks, so the leak oracle reads flat, and no count
reaches zero early, so guardfree unmaps nothing to fault on. What catches it is a
debug-only walk of every finished block
(`lir::lower::assert_cells_outlive_their_readers`), which runs in any debug build over
every block it lowers — so `cargo test` and a debug corpus run cover it and a release
`make smoke` does not. Two mechanisms hold one half of the order each and neither can
see the other, which is why the claim is stated once more over the finished emission.

## The Rust suite

`make test` runs the Rust suite after `make smoke`. For what kind of Rust test
to write and where, see [tests/AGENTS.md](../tests/AGENTS.md). `elle test
--rust`, which folds the cargo suite into the same store, is design only.

**Symbol names in assertions.** A name lives in the owning instance's display
memo, not in a global table ([docs/impl/symbol.md](impl/symbol.md)), and `fmt`
cannot reach it. So
a bare `{:?}`/`{}` on a symbol-bearing `Value` renders `#<symbol:hash>`. To read
names in assertion output, thread the table:
`format!("{}", v.display_with(Some(&symbols)))`, which renders the bare `name`
with no leading `'` (matching Scheme/CL). Comparing a symbol against a known
name needs no table and no formatting at all — use
`v.as_symbol() == Some(SymbolId::of("map"))`.

## Known gaps

- **No cross-file parallelism inside one runner** — the runner maps over files
  sequentially, so the gate targets deal the files into batches and run the
  batches side by side. Fanning out inside the runner (single SQLite writer)
  is the next perf step.
- **No history pruning** — the DB grows unbounded; `--prune` is specced,
  not implemented.

## See also

- [docs/test-vision.md](test-vision.md) — the plan that folds every test product into `elle test`.
- [docs/threads.md](threads.md) — worker threads, `os/spawn`, the scheduler the runner ships into workers.
