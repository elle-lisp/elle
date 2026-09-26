# Driving the test runner

<!-- audited: 2026-09-29 -->

Why `elle test` exists, the command line it offers, what it refuses to
offer, and what is still design.

How a run executes is [test-runner](test-runner.md); where it is stored is
[test-store](test-store.md).

> Status: **partially built** — these three documents are the specification,
> and its core is implemented in [src/test](../src/test) as the `elle test`
> subcommand (the `smoke-lang` and `smoke-impl` gates). Built (v1): per-file
> compilation, the per-form fault barrier and the whole-file mode,
> worker-thread isolation, the persistent SQLite
> index (a **subset** of the schema — see the note there), the per-run code
> state (commit, tree hash, worktree, host, build, boot fingerprint), the
> per-form analysis columns (`caps`, `touches`, `signal`), the on-disk CAS for
> stdout/stderr, run honesty (a killed run reads `DID NOT COMPLETE`), `:gated`
> skips, child-process isolation with the measurement channel it carries, the
> merge of another store's runs, and the
> `--query`/`--summary`/`--reset`/`--promote`/`-e`/`--timeout`/`--wide`/
> `--wide-timeout`/`--budget`/`--corpus`/`--db`/`--isolate`/`--host`/`--import`
> flags.
> Still design (not built): semantic selection
> (`--touches`/`--caps`/`--impacted-by`/`--changed`/`--rerun-failed`/`-k`),
> `--rust`/`--watch`/`--prune`/`-N`/`--format`, the per-run RSS/CPU capture,
> `--dump`/`--trace` asset capture, and `changed_file` population. A section
> marked "(v1, implemented)" / "(implemented)" / "**Resolved (v1)**" is built;
> the rest is the target.

## The problem this solves

The Elle-script harness the runner replaced (the corpus run through GNU `parallel`
from the [`Makefile`](../Makefile)) was CI-first and human-second. It was hostile
to an agent in specific, mechanical ways:

1. **The unit is the file, not the form.** `tests/lang/chan.lisp` contains ~30
   `(assert …)` cases but reports one bit via exit code. Because `assert` emits
   a signal that aborts the file, a run surfaces *one* failure even when ten are
   broken. An agent's fix loop is therefore serialized: fix, rerun, see the next
   failure, repeat.

2. **The authoritative record is terminal scrollback.** Failures are prose on
   stderr behind a `parallel --tag` prefix, and `--halt now,fail=1` kills the
   run before it completes. This produces the infuriating loop:

   ```
   log=$(mktemp); make smoke > "$log" 2>&1; tail -60 "$log"
   # not enough context — run again with grep
   # still not enough — run again with --trace
   # still not enough — run again with --dump=lir
   ```

   Every one of those re-runs recomputes the entire suite to recover information
   that the *first* run already had and threw away.

3. **Metadata lives out-of-band.** Which tiers a test supports and why it's
   skipped live in `Makefile` grep patterns (`ELLE_SKIP_VM`, `WASM_SKIP`, …),
   decoupled from the test. [`AGENTS.md`](../AGENTS.md) declares "no skip lists"
   as policy while the Makefile carries them.

4. **Elle's actual superpower is unused.** The whole thesis of the project —
   the compiler exposes structured truth about code via `compile/analyze`,
   Portrait, and the MCP graph — is ignored by the test harness, for selection,
   for failure context, and for the backend matrix.

## The thesis

**Capture everything once; query forever.** A run is not a stream of text that
scrolls past — it is a transaction against an append-only **SQLite results
database**. Every fact an agent might want after the fact — which forms failed,
their source, the emitted signal, the compiler artifacts (`--dump`), the runtime
trace (`--trace`), compile stats, resource consumption, the git state the run
ran against — is recorded the first time. The agent then issues **SQL**, never a
re-run. The terminal output is a convenience view layered on top of the
database; it is never the source of truth.

This directly kills the tail/grep/rerun loop: the complete record survives
truncation because it was never in the terminal to begin with, and the things an
agent would normally re-run *with special flags to obtain* (`--dump=lir`,
`--dump=stats`) are already captured in the local store.

## Non-goals / constraints (decided)

- **No new test form.** There is no `deftest`, no `check=`, no suite-metadata
  DSL — the existing `(assert cond "msg")` idiom is the whole vocabulary;
  introducing one means we missed the point. Reorganizing the corpus (one form
  per file, flat — [test-store](test-store.md)) is mechanical and preserves every
  form verbatim: it changes *where* forms live, never *how* they're written.
- **The computer names tests.** Identity is *derived*, never written. Naming is
  the runner's job, not the test author's.
- **Skip-lists die.** A backend-specific test gates itself where it lives
  ([test-runner](test-runner.md)), so the `Makefile` grep skip-lists
  disappear.
- **Elle drives `cargo`, not the reverse.** No cargo harness drives the `.lisp`
  suites — `elle test` does. A file that needs a runtime mode, such as
  `--trace=guardfree`, names it in a sidecar and runs on the rig as its own
  child ([rig](../rig/overview.md)), so its verdict lands in the database like
  any other. The remaining dependency to invert is the other direction: the
  runner *invoking* `cargo` for the Rust suite (`--rust`), folding its results
  into the same run — still future work.

## CLI surface

```
elle test [paths...]            # run each path once on this build's runtime, write DB
                                # (no --tiers flag — the builds are the tier set)
  -e 'FORM'                     # run an ad-hoc form; persist it in the index
  --promote ID [name]           # render ad-hoc syntax to <corpus>/<name>.lisp (flat; name suggested from analysis)
  --corpus DIR                  # durable corpus root to scan and promote into (default tests/)
  --changed                     # incremental: skip unaffected forms
  --rerun-failed                # only last run's failures
  --touches BINDING             # semantic selection
  --caps CAP
  --impacted-by REF
  -k SUBSTR                     # filter by derived label
  --rust                        # also invoke cargo; fold results into this run
  --format pretty|ndjson|summary  # terminal VIEW only — DB is always written
  --watch                       # stream results live as they land; exit with the gate code at completion
  --reset                       # remove the DB: clears ad-hoc + history
  --query 'SQL'                 # convenience: run SQL against the DB and exit
  --summary                     # re-print the latest run's tally and problems; no re-run
  --db PATH                     # session DB path, overriding the state directory
  --import PATH                 # merge another store's runs into this one; no run
  --isolate 'FLAGS'             # run each path as its own process: elle FLAGS PATH
  --host PROGRAM                # the program an isolated child runs, instead of this elle
  --timeout MS                  # per-form wall-clock budget (default 60000)
  --wide PATTERN                # a path substring whose forms take --wide-timeout (repeats)
  --wide-timeout MS             # the budget a wide path's forms get (default: --timeout)
  --budget                      # print each named path's budget in ms, then exit; no run
  --prune POLICY                # explicit history pruning (e.g. --prune adhoc)
  -N                            # stop after N failures (-1 = fail-fast); default: run to completion
```

These global `elle` flags pass through to the runner's own VM rather than
being read as corpus paths: `--trace=...`, `--boot-image=...`, and
`--dump=stats`. The I/O backend is a property of the build, so a pool-only
wedge is chased on a Linux box by building without the `uring` feature
([config](config.md) § Builds).
`--boot-image=` boots the runner from an image, so every corpus file is
compiled against a hydrated stdlib rather than a freshly compiled one
([boot](impl/image/boot.md)).

### The budget follows the file

`--timeout MS` is one number for every form in a run, and the corpus does not
hold one kind of file. The `h2-bidi-`, `h2-load-`, `h2-stream-` and
`h2-timeout-` families drive hundreds of requests or streams over one session,
and each file carries its own `deadline`. That deadline is what says which
request stalled and how long it waited, and it prints only if the runner's
budget outlasts it. A file killed at the narrower budget reports `timeout` and
nothing else, which reads as a slow runner rather than as the stall it found.

So the budget follows the path, not the run. `--wide PATTERN` names a path
substring, and a path holding any named substring gives its forms
`--wide-timeout MS` instead of `--timeout MS`. The [`Makefile`](../Makefile)
already writes that family list down for the one-process-per-file passes; it
names the families once and hands the same list to both budgets, so a family
added there widens in both.

`--budget` answers rather than runs. It prints the budget each named path would
get, in milliseconds, one path per line, and exits zero without touching the
session store. [budget.rs](../tests/integration/budget.rs) reads the runner's
own selector through it rather than restating the rule in Rust.

### A run recorded elsewhere reads like a local one

A CI job's results are a file, and a file nothing merges is a database you
point `--db` at, losing the join against every local run. `--import PATH` takes
the foreign session DB, appends its runs to this store with their identity
intact, and copies the CAS bytes its assets name
([test-store](test-store.md)).

The import records no run of its own and runs no test, so it exits zero on a
store whose runs failed. What those runs say is then a query, exactly as for a
run recorded here.

### Execution and completion

By default `elle test` **runs to completion** and collects every result — it does
*not* stop at the first failure (the opposite of today's `--halt now,fail=1`).
This follows from the thesis: the value is the **complete failure set in one
shot** (fix everything in one pass, not fix-one-rerun-see-next), a completed run
leaves a **complete DB** (query instead of re-running), and the exit code is a
clean gate — zero iff every selected form passed.

The agent controls the completion policy, three ways:

- **(default) run to completion** — full set, full DB, gate exit code. The
  blocking call returns once everything is recorded.
- **`-N` stop-after-N** — `-1` is fail-fast (stop at the first failure) for a fast
  yes/no; larger `-N` stops once N failures are recorded, when you want more than
  the first but not the whole run. Scheduling stops at the Nth failure; in-flight
  forms finish and land in the DB, so even an early stop isn't information-lossy
  the way a traditional fail-fast is.
- **`--watch`** — stream each result to the terminal as it lands (the attached,
  live view of an otherwise-quiet blocking run), so the agent acts on the
  earliest failure **without aborting the rest**. It detaches when the run
  completes, exiting with the gate code — it does *not* keep tailing the session
  (a persistent feed buys nothing the DB doesn't already give). Whether that exit
  code arrives synchronously or via a completion notification is just
  foreground-vs-background — the caller's choice, not a runner mode.

Once results are persisted incrementally, "first actionable event" and "run to
completion" stop competing.
The suite finishes (the DB fills) while the agent is already working the earliest
failure. And because the runner owns execution order
([test-store](test-store.md)), the
default order is **failure-likely-first** — forms that failed recently or whose
dependency closure just changed run before the rest — so even a run-to-completion
pass surfaces the most probable failures early in wall-clock. The first run of a
fresh branch has no history to order by, so it simply runs everything in scan
order; the prioritization kicks in once results exist.

## Selection (uses `compile/analyze`)

Selection narrows *which forms* run
([test-runner](test-runner.md)). These are **inner-loop accelerators**,
not the gate. Any filtered run
records its predicate in `run.selection`, so a partial run is visibly partial and
cannot be passed off as a full green. The gate — what CI, the merge queue, and a
"done" claim require — is a `selection IS NULL` run: every form.

Because the runner holds forms as data and analyzes them before eval, selection
is semantic, not just glob/name:

| Flag | Selects |
|------|---------|
| `tests/lang/chan.lisp` | a file (positional) |
| `-k SUBSTR` | forms whose derived label matches |
| `--touches chan/send` | forms whose analysis references that binding |
| `--caps io` | forms that exercise an I/O capability |
| `--impacted-by <ref>` | forms whose dependency closure intersects the diff |
| `--changed` | forms whose source + dependency closure are untouched since their last green result are skipped (fast inner loop) |
| `--rerun-failed` | the failing set from the last run (read from the DB) |

`--changed` is the incremental payoff of the datastore: `form.hash` plus
`caps`/`touches` from `compile/analyze` plus the run's `changed_file` set tell
the runner exactly which forms can be safely skipped. Full run stays the default.

### `--rust`: folding in the cargo suite

`elle test --rust` invokes `cargo test --message-format=json`, parses libtest's
JSON, and writes one `result` row per Rust test with `tier='rust'`, capturing
cargo's stdout/stderr as assets under the same `run`. The whole suite — Elle
behavioral, differential, and Rust — lives in one queryable transaction. (If the
libtest JSON format turns out to be unstable on the toolchain in use, fall back
to parsing the human output — but it's expected to hold.)

## Substrate

The runner is an **Elle program**, living in [src/test](../src/test) and
surfaced as the `elle test` subcommand alongside `elle fmt`/`elle lint`. Its
files are fragments of one module, concatenated in order by
[main.rs](../src/main.rs) and embedded in the binary, so a run needs no source
tree. It is built from machinery the
language already exposes — the **file-compilation pipeline** (plus the per-form
fault-barrier compilation mode, [test-runner](test-runner.md)),
`compile/analyze`, `lib/sqlite.lisp`, `std/compress` (zstd for the CAS), and
`read-all` for ad-hoc `-e` snippets — plus two additions of its own: the
`gate!` gating macro, and the in-process artifact-capture compile option,
realized as the
`(compile/dumps SRC NAME)` primitive ([test-store](test-store.md)) that returns
the `--dump` artifact set as strings rather than printing them and exiting.

The run's code state comes from `git` and `uname` through `subprocess/exec`,
and the binary's own identity from `(elle/version)`, `(elle/build-profile)`,
`(elle/executable)` and `(elle/boot-fingerprint)`. None has another source:
each is a fact about this binary, so only this binary can report it. The
executable path is what `--isolate` spawns unless `--host` names another —
a child resolved off `PATH` would be a different build, and the run would say
nothing about the one under test.
The fingerprint hashes that executable, which carries the sources it boots
from ([test-store](test-store.md) § The boot fingerprint).

## Open implementation questions (for the tests/code phases)

- `(clock/cpu)` granularity and whether per-form deltas are meaningful once
  the JIT has compiled a form (it may run in sub-microsecond territory).
  Decide whether to record CPU per-form, per-file, or only run-level.
- Concurrency: the current harness gets parallelism from GNU `parallel` across
  files. The new runner parallelizes across forms/files internally (worker
  threads, [test-runner](test-runner.md)) while keeping SQLite writes
  serialized (single writer, WAL).
- The `%assert` intrinsic's elision rules: when may the analyzer drop the
  syntax-capture (provably-true predicate, assertions-disabled build) without
  changing observable behavior for tests that *expect* a failure signal?
- The gating macro `gate!`: whether it ever chooses compile-time elision over a
  runtime guard, and whether `:gated` is a registered signal bit or a plain user
  keyword (it changes the signal profile of any function using the loud gate).
- `form.hash` over read Syntax with comments elided is the default; confirm the
  Syntax representation actually drops comment trivia (or strip it explicitly
  before hashing).
- The per-form fault-barrier compilation mode: how a top-level form's signal is
  caught and the next form resumed *within one compiled module* without nesting
  forms in lambdas (which would break top-level binding scope). **Resolved (v1):**
  the file is compiled once through `analyze_file_letrec`; `def`/`var` forms run
  eagerly to establish shared bindings while each test form is reified as a thunk
  capturing that environment; the runner runs each thunk with the fault barrier
  *outside* the thunk. [test-runner](test-runner.md) holds the full mechanism
  and its intentional boundaries.
