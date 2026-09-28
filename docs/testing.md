# Testing

<!-- audited: 2026-09-29 -->

The two test suites, what each one claims, the builds that run them, and how a
run is read.

Elle has a language suite and an implementation suite
([spec](spec.md) § Two suites):

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
| `make smoke-lang` | The language suite, each file as its own `elle FILE` |
| `make smoke-impl` | The implementation suite on the rig, then both suites under each rig profile |
| `make smoke` | Both suites, the doctests, the embedding demo, and the surface gate |
| `make test` | `make qa`, then `make smoke`, then the Rust unit and integration tests |
| `elle test tests/lang/*.lisp` | Run those files in-process; print a summary; gate on exit code |
| `elle-rig tests/impl/NAME.lisp` | Run one implementation test with its sidecar |
| `elle test --summary` | Re-print the last run's summary (no re-run) |
| `elle test --query 'SQL'` | Run ad-hoc SQL |

A run prints a tally, a line per failure, and what it cost the runner's own
heap — all to stderr:

```
elle test · run 7 of 7 · commit a1b2c3d (dirty)
184 pass · 6 skip · 1 fail · 1 timeout
2 problems (query the DB for full detail):
  fail     tests/lang/foo.lisp:12  [process]  expected 42, got 41
  timeout  tests/lang/subprocess.lisp  [process]  child exceeded the 60000 ms budget
runner heap · objects +9021 · regions +28104 · pages +112
  objects +4510  regions +14052  pages +56  tests/lang/a.lisp
```

The commit line names the code the tally describes. A run outside a
repository prints the run number alone. The `runner heap` block is the run's
account of what it cost itself, file by file
([docs/test-store.md](test-store.md) § The runner's own gauges).

You read results from the run itself — never by hand-writing SQLite.

## A build is an implementation

A build carries one optimizing tier and no flag that chooses another
([config](config.md) § Builds). So tier coverage is not a setting of a run: it
is the set of builds that run the language suite. CI builds the default
(JIT) build, a build with no JIT, a thread-pool I/O build, an MLIR build, a
build with no features, and the default build on AArch64 and macOS, and each
runs `make smoke-lang` ([ci](analysis/ci.md)). A file that passes on one build
and fails on another has found a defect in the build that fails.

The implementation suite runs on the default build and its rig. It adds what
no build ships: both suites with every function compiled on its first call,
and on macOS the language suite with each released page scrubbed. It also runs
on the thread-pool build's rig, where the worker threads it counts exist.

## The agent-first runner (`elle test`)

The runner compiles and runs files in one process, recording every result into
a **SQLite DB** plus a filesystem CAS for artifacts. The thesis (see
[docs/test-cli.md](test-cli.md)): *capture everything once; query forever* —
so an agent issues SQL against the stored run instead of re-running with
`--dump`/`--trace`.

- **The suites are the source of truth, in git.** The DB is a derived index
  living outside the repo, in the state directory: `$ELLE_STATE`, else
  `$XDG_STATE_HOME/elle`, else `$HOME/.local/state/elle`
  ([docs/test-store.md](test-store.md) § Run history is state). Run history
  is a record, so it does not live with the caches a rebuild regenerates.
  `--db PATH` moves the database, its CAS, and its scratch files together.
- **The DB tracks all runs.** Each invocation appends a `run` row; `--summary`
  shows the *latest* run (`run N of M` makes the history visible).

### How a file is run

The unit is the **file**, compiled the way every real Elle program is — Source →
Reader → … → Bytecode → VM, with whole-module analysis — not `read`+`eval`'d
form-by-form. The runner runs each file **once**, under the runtime its build
ships, and records one `worker` row: the tier the runtime picks for each
function is the runtime's business, exactly as it is under `elle FILE`.

Test code is untrusted, so each file runs in a **worker thread** with its own VM,
bounded by the budget its path earned — `--timeout MS` (default 60000), or
`--wide-timeout MS` for a path `--wide` names
([docs/test-cli.md](test-cli.md)); a form that never finishes is recorded
`timeout`. A `(exit)` inside a test is **trapped** (it would otherwise terminate
the whole run): `exit 0` is recorded `skip`, any other code `fail`. A worker that
can't host a thunk (an unsendable FFI/fiber capture) falls back to in-process
execution.

### A file as its own process

`elle test --isolate 'FLAGS'` runs each path as `elle FLAGS PATH`, one child per
path, recorded on the `process` tier. `--host PROGRAM` runs the child under
another program instead of this `elle`, which is how the implementation suite
runs on the rig. The gate targets run both suites this way: each file then
starts, runs as a whole program and exits, which is the only shape that covers
program teardown, and a fault in one file kills one child rather than the run.

A child that dies on a signal is a `fail` naming the signal and the run
continues; an exit code is a `fail` naming the code; a child over its budget
is killed and recorded `timeout`. Its stdout and stderr become assets either
way ([docs/test-runner.md](test-runner.md) § Isolation).

```sh
elle test --isolate '' tests/lang/closures.lisp
elle test --host target/release/elle-rig --isolate '' tests/impl/oracle.lisp
```

An isolated child also carries the **measurement channel**: a dashboard that
reports a verdict through it — [oracle.lisp](../tests/impl/oracle.lisp) and
[plumb.lisp](../tests/impl/plumb.lisp) do, through
[estimator.lisp](../tests/impl/lib/estimator.lisp) — lands one `measurement`
row per verdict, so a leak rate's history across commits is a query rather than
scrollback ([docs/test-store.md](test-store.md) § Measurements). Run the same
file directly and it prints its dashboard and records nothing.

That is how the Makefile runs the two dashboards. They belong to the
implementation suite, so every pass over that suite runs each one as an
isolated child on the rig, under the wide budget that `WIDE_FAMILIES` names.

### Statuses

| Status | Meaning | Gates? |
|--------|---------|--------|
| `pass` | the file ran to its end | no |
| `skip` | gated out (`gate!`/`:gated`), or `(exit 0)` | no |
| `fail` | an assertion or error | **yes** |
| `timeout` | the file exceeded its budget | **yes** |

The gate (exit code) is zero iff no form failed or timed out. `status`
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
([rig](../rig/overview.md) § The sidecar).

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
([modules.md](modules.md) § "Module search path"); a written-out
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
than the regression it exists to catch. `tests/impl/bytes-linear.lisp` failed
at 0.5055s against a 0.5 bound, on a commit that costs 0.003s on a quiet box.

Measure a **control** instead. Pick an operation of the same size, in the same
process, that runs the path the regression cannot reach, and require the
subject to stay within a small multiple of it. A starved machine slows both
measurements, so the ratio survives what an absolute bound does not, and a
per-byte path costs about a hundred times the bulk one.

Two rules keep the ratio steady:

- **Alternate the two measurements, and keep the smallest of several rounds.**
  Both thunks then sample the same stretch of machine, and one stalled round
  decides nothing.
- **Build the operands outside the timed thunk.** `length` on a string counts
  graphemes, so a loop that re-reads it times the walk instead of the work.

The two worked examples are `tests/impl/bytes-linear.lisp`, which gates a
binary append against the same-size text append, and
`tests/impl/concat-linear.lisp`, which gates a string concat against the
same-size bytes concat.

A **timeout** test is the other case, and it keeps its absolute bound. There
the deadline is the specification: a 50 ms `chan/select` has to return in about
50 ms, and no ratio can say what that means.

### Per-thread native teardown

An FFI library may register thread-local destructors — libgit2 does, through
OpenSSL's `pthread_key_create`. If a worker that used such a library `dlclose`d it on
teardown, glibc would later run the destructor — at worker thread exit — into the
unmapped code, killing the process with SIGSEGV in `__nptl_deallocate_tsd`.

This is closed by construction: FFI library mappings are owned **process-globally**
and **never `dlclose`d** (`src/ffi/registry.rs`; the same discipline plugins use),
so a worker that uses an FFI library and exits is always safe — the destructor runs
against still-mapped code. No per-worker teardown is required. A program may attach
an *optional, explicit* ordered teardown to a library with `(ffi/on-unload lib
"sym")` and run them with `(ffi/run-teardowns)` (`lib/git.lisp`'s `git:shutdown`, for example);
these are graceful cleanup the program triggers when its worker threads have
quiesced, never run automatically and never required for safety. Pinned by
`tests/integration/ffi_worker.rs` (a worker that loads a TLS-destructor fixture and
exits without teardown exits cleanly).

## Reading a run

```sh
elle test --summary                       # latest run's tally + problems
elle test --query 'SELECT * FROM run'     # run history
elle test --query \
  "SELECT f.file, r.tier, r.reason FROM result r
   JOIN form f ON f.hash = r.form_hash WHERE r.status = 'fail'"
```

The schema (`run`, `form`, `result`, `asset`, `measurement`, `gauge`,
`changed_file`) is documented in
[docs/test-store.md](test-store.md) § Schema (with the v1 implemented-subset
note — the `run` resource columns are deferred). Each `run` row names the code
it ran against — commit, dirty flag, tree hash, worktree — and the binary and
machine that ran it, so a result belongs to something and the killed-run
warning names the checkout it warns about. Captured stdout/stderr
live in the CAS at `<db-dir>/cas/<hash>`, referenced by `asset` rows. `--dump`
artifact capture (the LIR-as-a-hash-lookup path) is currently **omitted** — it
OOMs the corpus run and does not dedup
([docs/test-runner.md](test-runner.md) § CAS asset capture) — so
today only stdout/stderr assets exist; the LIR of a failing form still needs a
re-run until that capture is re-enabled.

What a run cost the runner's own heap is recorded per file, in objects,
regions and pages, and the summary names the files that grew it most. A leak
per compiled file used to reach us as an OOM kill and a batch size; now it
reaches us as a file name and a number
([docs/test-store.md](test-store.md) § The runner's own gauges).

```sh
elle test --query \
  "SELECT file, sum(delta) AS regions FROM gauge
   WHERE kind = 'regions' GROUP BY file ORDER BY regions DESC LIMIT 10"
```

A form that misses its deadline prints a native backtrace of every thread in
the runner process to stderr, under `── threads at the deadline ──`. `sys/join`
abandons a timed-out worker rather than killing it — an OS thread cannot be
safely killed — so the wedged thread is still parked in whatever call stopped
it, and this reads its stack while that is still true. It is what separates a
hang from a slow test, and it is the only account available for a form that
hangs on one machine and nowhere else: re-running the file cannot stand in for
it, because the runner puts each form on its own worker thread and a hang that
needs that thread does not reproduce under a plain run.

The sampler is `sample` on macOS and `eu-stack` on a Linux box with elfutils. A
box with neither prints nothing and the run is unaffected; a passing form never
pays for it.

A run killed mid-flight (OOM, signal) is recorded honestly: its `run` row's
`finished_at` stays NULL, `--summary` labels it `DID NOT COMPLETE` with the live
partial tally (computed from `result` rows — the stored counters are written
only at completion), and the next `elle test` warns about it. An all-pass
result set from a truncated run is partial coverage, not green
(see [docs/test-runner.md](test-runner.md) § Run honesty). A run that is still
working leaves the same NULL, so the views ask whether its process is alive:
`--summary` then reads `STILL RUNNING (pid P)`, and the next `elle test` prints
one line naming the pid instead of the kill warning.

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
assertions, not by a memory gauge: the `tests/lang/recur-after-yield.lisp`,
`recur-after-tail-call.lisp`, and `recur-as-value.lisp` language tests (run on
every build), with deterministic peers in `src/runtime/tests/selfrec.rs`. Each
asserts a result that is only correct if the self-identity survived the
boundary, so a stale self-reference flips the assertion red. They are the
regression guard for any change to how a self-reference is resolved or how an
activation is carried across yield, tail call, or value handoff.

The **order of two correctly-counted releases** is the other hazard of this kind, and
it needs a third detector rather than a behavioral pin. A captured binding's value and
its env cell are two regions addressed by one env index; the value's release loads the
box raw and unwraps it, so it reads the page the box's release frees
([docs/impl/region/cells.md](impl/region/cells.md) § "A cell's release lands at
or after every release routed through that cell"). Emit the two in the wrong order and
both counts are still right: nothing leaks, so the leak oracle reads flat, and no count
reaches zero early, so guardfree unmaps nothing to fault on. What catches it is a
debug-only walk of every finished block
(`lir::lower::assert_cells_outlive_their_readers`), which runs in any debug build over
every block it lowers — so `cargo test` and a debug corpus run cover it and a release
`make smoke` does not. Two mechanisms hold one half of the order each and neither can
see the other, which is why the claim is stated once more over the finished emission.

## The Rust suite

`make test` runs `make qa` first — `cargo fmt --check`, clippy,
`make crosscheck`, rustdoc — then the corpus, then `cargo test --lib` and the
integration tests. For what kind of Rust test to write and where, see [tests/AGENTS.md](../tests/AGENTS.md) and
[docs/analysis/testing.md](analysis/testing.md). (`elle test --rust`, which folds
the cargo suite into the same DB, is specced but not yet implemented.)

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

- [docs/spec.md](spec.md) — the two suites, and what the specification holds.
- [rig/overview.md](../rig/overview.md) — the rig, its sidecars and its profiles.
- [docs/test-runner.md](test-runner.md) — how a run executes: compilation, isolation, gating, honesty.
- [docs/test-store.md](test-store.md) — where a run is stored, what it records, and the schema.
- [docs/test-cli.md](test-cli.md) — why the runner exists, its command line, and what is still design.
- [docs/test-vision.md](test-vision.md) — the plan that folds every test product into `elle test`.
- [tests/AGENTS.md](../tests/AGENTS.md) — Rust test categories, helpers, fixtures.
- [docs/analysis/testing.md](analysis/testing.md) — the decision tree.
- [docs/threads.md](threads.md) — worker threads, `os/spawn`, the scheduler the runner ships into workers.
