# Testing

Elle has two test systems:

1. **The Elle corpus** — `.lisp` files under `tests/elle/`, run through the
   **agent-first runner** (`elle test`). This is what `make smoke` gates on.
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
| `make test` | `make smoke` + Rust fmt/clippy/crosscheck/rustdoc/unit/integration |
| `make crosscheck` | Clippy the macOS `cfg(target_os)` arms from Linux (no SDK needed) |
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
  values, the runner records a synthetic `diverge` row — this *is* the
  differential (cross-tier) testing path (docs/impl/differential.md).
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
[`docs/test-runner.md`](test-runner.md) § Schema (with the v1 implemented-subset
note — the `run` code-state/resource columns are deferred). Captured stdout/stderr
live in the CAS at `<db-dir>/cas/<hash>`, referenced by `asset` rows. `--dump`
artifact capture (the LIR-as-a-hash-lookup path) is currently **omitted** — it
OOMs the corpus run and does not dedup (test-runner.md § CAS asset capture) — so
today only stdout/stderr assets exist; the LIR of a failing form still needs a
re-run until that capture is re-enabled.

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
(see [`docs/test-runner.md`](test-runner.md) § Run honesty).

## Correctness the leak and UAF oracles cannot see

The two automated memory oracles each have a blind spot. `tests/elle/oracle.lisp`
measures heap *growth* — in objects, regions, bytes, or physical region ids — so it
sees a leak, never a wrong answer. `--trace=guardfree` faults on a *use-after-free*
— it sees a dangling read, never a live read of the wrong live value. A computation
that returns a **wrong-but-well-typed value** slips past both: no region leaked, no
freed page was touched, the result is just silently incorrect.

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

The **order of two correctly-counted releases** is the other hazard of this kind, and
it needs a third detector rather than a behavioral pin. A captured binding's value and
its env cell are two regions addressed by one env index; the value's release loads the
box raw and unwraps it, so it reads the page the box's release frees
([`docs/impl/region/bindings.md`](impl/region/bindings.md) § "A cell's release lands at
or after every release routed through that cell"). Emit the two in the wrong order and
both counts are still right: nothing leaks, so the leak oracle reads flat, and no count
reaches zero early, so guardfree unmaps nothing to fault on. What catches it is a
debug-only walk of every finished block
(`lir::lower::assert_cells_outlive_their_readers`), which runs in any debug build over
every block it lowers — so `cargo test` and a debug corpus run cover it and a release
`make smoke` does not. Two mechanisms hold one half of the order each and neither can
see the other, which is why the claim is stated once more over the finished emission.

## The Rust suite

`make test` runs the Rust gate after the corpus: `cargo fmt --check`, clippy,
`make crosscheck`, rustdoc, `cargo test --lib`, and the integration tests. For
what kind of Rust test to write and where, see [`tests/AGENTS.md`](../tests/AGENTS.md) and
[`docs/analysis/testing.md`](analysis/testing.md). (`elle test --rust`, which folds
the cargo suite into the same DB, is specced but not yet implemented.)

**Symbol names in assertions.** Symbol name resolution is per-instance
(docs/impl/region/ctx.md § "Symbols through the ctx"). So a
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
