# Testing

Elle has two test systems:

1. **The Elle corpus** — `.lisp` files under `tests/elle/` and `tests/diff/`, run
   through the **agent-first runner** (`elle test`). This is what `make smoke`
   gates on.
2. **The Rust suite** — unit, integration, and property tests under `tests/` and
   inline `#[cfg(test)]` modules, run through `cargo test`. See
   [`tests/AGENTS.md`](../tests/AGENTS.md) for categories, helpers, and how to add
   one, and [`docs/analysis/testing.md`](analysis/testing.md) for the
   decision tree (which kind of Rust test to write).

This document covers the Elle corpus and the runner; the runner's full
specification is [`docs/test-runner.md`](test-runner.md).

## Quick start

| Command | What it does |
|---------|--------------|
| `make smoke` | The corpus through `elle test` + doctests + the embedding demo |
| `make test` | `make smoke` + Rust fmt/clippy/rustdoc/unit/integration |
| `elle test tests/elle/*.lisp` | Run those files; print a summary; gate on exit code |
| `elle test --summary` | Re-print the last run's summary (no re-run) |
| `elle test --query 'SQL'` | Run ad-hoc SQL |

A run prints a tally and a line per failure to stderr, e.g.:

```
elle test · run 7 of 7 · 184 pass · 6 skip · 1 fail · 0 diverge · 1 timeout
2 problems (query the DB for full detail):
  fail     tests/elle/foo.lisp:12  [jit]  expected 42, got 41
  timeout  tests/elle/subprocess.lisp  [vm]  join: deadline exceeded
```

You read results from the run itself — never by hand-writing SQLite.

## The agent-first runner (`elle test`)

The runner compiles and runs the **whole corpus in one process**, recording every
`(form × tier)` result into a **SQLite DB** plus a filesystem CAS for
artifacts. The thesis (see [`docs/test-runner.md`](test-runner.md)): *capture
everything once; query forever* — so an agent issues SQL against the stored run
instead of re-running with `--dump`/`--trace`.

- **The corpus is the source of truth, in git.** The DB is a derived, rebuildable
  index living outside the repo (default `$ELLE_CACHE/elle-tests.db`).
- **The DB tracks all runs.** Each invocation appends a `run` row; `--summary`
  shows the *latest* run (`run N of M` makes the history visible).

### How a file is run

The unit is the **file**, compiled the way every real Elle program is — Source →
Reader → … → Bytecode → VM, with whole-module analysis — not `read`+`eval`'d
form-by-form. Two shapes:

- **A single-form file** (one top-level expression — the durable corpus shape) is
  forced onto **every backend tier** via `compile/run-on` (`vm`, `jit`, and
  `wasm`/`mlir-cpu` when the build carries them). If two tiers return *different*
  values, the runner records a synthetic `diverge` row — this folds the old
  `tests/diff/` differential tests into the same path.
- **A legacy multi-form file** (most of `tests/elle/`) is an imperative script, so
  it is wrapped as one whole-file thunk and run under each **JIT policy**: once
  with `jit=off` (recorded tier `vm`) and once with `jit=eager` (recorded tier
  `jit`). This is exactly the old `smoke-vm` + `smoke-jit` split, in one
  invocation. No cross-tier value divergence is judged for these (a script's pids
  and timestamps differ run-to-run by design).

Test code is untrusted, so each file runs in a **worker thread** with its own VM,
bounded by `--timeout MS` (default 60000); a form that never finishes is recorded
`timeout`. A `(exit)` inside a test is **trapped** (it would otherwise terminate
the whole run): `exit 0` is recorded `skip`, any other code `fail`. A worker that
can't host a thunk (an unsendable FFI/fiber capture) falls back to in-process
execution.

### Statuses

| Status | Meaning | Gates? |
|--------|---------|--------|
| `pass` | the form returned a value | no |
| `skip` | gated out (`gate!`/`:gated`), tier-ineligible, or `(exit 0)` | no |
| `fail` | an assertion or error | **yes** |
| `diverge` | tiers returned different values (synthetic `tier='*'` row) | **yes** |
| `timeout` | the form exceeded `--timeout` | **yes** |

The gate (exit code) is zero iff no form failed, diverged, or timed out. `status`
and `tier` are keyword-valued in the runner and stored as their bare name in the
TEXT columns (`WHERE status = 'pass'` works as written).

## Adding an Elle test

Write a `.lisp` file under `tests/elle/` using the one idiom — `(assert COND
"message")`. There is no `deftest`, no suite DSL. The runner scavenges the form's
first `assert` message as the test's label.

```lisp
(elle/epoch 11)
## what this file checks
(assert (= (+ 1 1) 2) "addition works")
```

### Gating, not skip-lists

A test that needs an optional dependency (an FFI library, a GPU, a running
service, a specific backend) **gates itself in-file** — there are no Makefile skip
lists for the runner. Re-raise a missing dependency as `:gated` so the runner
records a reasoned `skip` (and a direct `elle FILE` run exits 0 cleanly):

```lisp
(def [ok? lib] (protect (import-file "target/release/libfoo.so")))
(unless ok?
  (error (struct :error :gated :reason "libfoo plugin not built")))
```

For backend-specific assertions, gate on the live policy/tier
(`(vm/config :jit)`, `(backend? :jit)`). **Never** `(exit 0)` to skip — under the
runner the trap turns it into a `skip`, but `:gated` carries a *reason* and is the
intended idiom.

### Per-thread native teardown

An FFI library may register thread-local destructors (e.g. libgit2 via OpenSSL:
`pthread_key_create`). If a worker that used such a library `dlclose`d it on
teardown, glibc would later run the destructor — at worker thread exit — into the
unmapped code, killing the process with SIGSEGV in `__nptl_deallocate_tsd`.

This is closed by construction: FFI library mappings are owned **process-globally**
and **never `dlclose`d** (`src/ffi/registry.rs`; the same discipline plugins use),
so a worker that uses an FFI library and exits is always safe — the destructor runs
against still-mapped code. No per-worker teardown is required. A program may attach
an *optional, explicit* ordered teardown to a library with `(ffi/on-unload lib
"sym")` and run them with `(ffi/run-teardowns)` (e.g. `lib/git.lisp`'s `git:shutdown`);
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

The schema (`run`, `form`, `result`, `asset`, `changed_file`) is documented in
[`docs/test-runner.md`](test-runner.md) § Schema. Artifact bytes (`--dump`
renderings, captured stdout/stderr) live in the CAS at `<db-dir>/cas/<hash>`,
referenced by `asset` rows — so the LIR of a failing form is a hash lookup, not a
re-run.

## Correctness the leak and UAF oracles cannot see

The two automated memory oracles each have a blind spot. `tests/elle/oracle.lisp`
measures region/object *growth* — it sees a leak, never a wrong answer.
`--trace=guardfree` faults on a *use-after-free* — it sees a dangling read, never a
live read of the wrong live value. A computation that returns a **wrong-but-well-typed
value** slips past both: no region leaked, no freed page was touched, the result is
just silently incorrect.

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
both oracles above. So this correctness is pinned **behaviorally**, by value assertions,
not by a memory gauge: the `tests/elle/recur-after-yield.lisp`,
`recur-after-tail-call.lisp`, and `recur-as-value.lisp` corpus files (run on every tier
and under `--trace=guardfree`), with deterministic peers in
`src/runtime/tests/selfrec.rs`. Each asserts a result that is only correct if the
self-identity survived the boundary, so a stale self-reference flips the assertion red.
They are the regression guard for any change to how a self-reference is resolved or how
an activation is carried across yield, tail call, or value handoff.

## The Rust suite

`make test` runs the Rust gate after the corpus: `cargo fmt --check`, clippy,
rustdoc, `cargo test --lib`, and the integration tests. For what kind of Rust test
to write and where, see [`tests/AGENTS.md`](../tests/AGENTS.md) and
[`docs/analysis/testing.md`](analysis/testing.md). (`elle test --rust`, which folds
the cargo suite into the same DB, is specced but not yet implemented.)

**Symbol names in assertions.** Symbol name resolution is per-instance
(docs/impl/region-ctx.md § "Symbols through the ctx"). So a
bare `{:?}`/`{}` on a symbol-bearing `Value` (as in `assert_eq!` output) renders
`#<sym:id>`, not the bare `name`, because the trait `fmt` has no table to thread.
When a failing assertion needs readable names, put `v.debug_with(symbols)` /
`v.display_with(symbols)` in the assert message — those carry the table (a symbol
then renders as its bare `name`, matching Scheme/CL — no leading `'`).

## Known gaps

- **`subprocess.lisp` hangs in a worker** — `subprocess/wait` relies on SIGCHLD,
  which is masked on worker threads, so the reaper never wakes; the runner records
  a `timeout`. It is quarantined from `make smoke` (`ELLE_TEST_SKIP` in the
  Makefile) until reaping moves to the main thread or such files route in-process.
- **No cross-file parallelism yet** — the runner maps over files sequentially
  (parallelism is per-form within a file), so a full corpus run is minutes, not
  seconds. Fanning out across files (single SQLite writer) is the next perf step.
- **Multi-form files don't get per-tier *divergence*** — they run under each JIT
  policy (vm/jit) but aren't value-diffed across tiers. Real cross-tier divergence
  needs the corpus migrated to one-form-per-file (the durable shape).
- **No history pruning** — the DB grows unbounded; `--prune` is specced,
  not implemented.

## See also

- [`docs/test-runner.md`](test-runner.md) — the runner's full specification and schema.
- [`tests/AGENTS.md`](../tests/AGENTS.md) — Rust test categories, helpers, fixtures.
- [`docs/analysis/testing.md`](analysis/testing.md) — Rust test decision tree.
- [`docs/threads.md`](threads.md) — worker threads, `os/spawn`, the scheduler the runner ships into workers.
