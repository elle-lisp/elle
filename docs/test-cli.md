# Driving the test runner

<!-- audited: 2026-10-22 -->

Why `elle test` exists, the command line it offers, what it refuses to
offer, and what is still design.

How a run executes is [test-runner](test-runner.md); where it is stored is
[test-store](test-store.md).

> Status: **partially built**. These three documents are the specification,
> and [src/test](../src/test) implements its core as the `elle test`
> subcommand, behind the `smoke-lang` and `smoke-impl` gates. Built:
>
> - per-file compilation, the per-form fault barrier and the whole-file mode
> - worker-thread isolation and child-process isolation
> - the persistent SQLite index, a **subset** of the schema (see the note there)
> - the per-run code state: commit, tree hash, worktree, host, build and boot
>   fingerprint
> - the per-form analysis columns `caps`, `touches` and `signal`
> - the on-disk CAS for stdout and stderr
> - each result's wall time and CPU time, and an isolated child's peak
>   resident set
> - run honesty: a killed run reads `DID NOT COMPLETE`
> - `:gated` skips
> - the readings every form prints, judged against the ledger of the run's
>   build ([ratchet](ratchet.md)), and `--repin`
> - the merge of another store's runs
> - the `--query`/`--summary`/`--reset`/`--promote`/`-e`/`--timeout`/`--wide`/
>   `--wide-timeout`/`--budget`/`--corpus`/`--db`/`--isolate`/`--host`/`--import`/`--charge`
>   flags
>
> Still design: semantic selection
> (`--touches`/`--caps`/`--impacted-by`/`--changed`/`--rerun-failed`/`-k`),
> `--rust`/`--watch`/`--prune`/`-N`/`--format`, the per-run RSS/CPU capture,
> `--dump`/`--trace` asset capture, and `changed_file` population.

## The problem this solves

A suite that reports through exit codes and terminal output is hostile to an
agent in four mechanical ways:

1. **The unit is the file, not the form.** A file of thirty `(assert …)` cases
   reports one bit. An `assert` aborts the file, so a run shows one failure when
   ten are broken, and the agent fixes them one rerun at a time.

2. **The record is terminal scrollback.** Failures are prose on stderr, and a
   fail-fast run stops before it completes. Each rerun for more context, with a
   grep, `--trace` or `--dump=lir`, recomputes the whole suite to recover what
   the first run had and threw away.

3. **Metadata lives out of band.** The tiers a test supports, and why it is
   skipped, live in `Makefile` grep patterns (`ELLE_SKIP_FFI`, `WASM_SKIP`),
   apart from the test. [CONTRIBUTING.md](../CONTRIBUTING.md) forbids skip
   lists while the Makefile carries them.

4. **The compiler's facts go unused.** The compiler states structured facts
   about code through `compile/analyze`, portraits and the MCP graph. A harness
   that ignores them cannot use them to select tests, explain a failure, or
   choose a tier.

## The thesis

**Capture everything once; query forever.** A run is a transaction against an
append-only **SQLite results database**, not a stream of text that scrolls
past. The run records every fact an agent might want later, the first time.
That covers which forms failed, their source, the emitted signal, the compiler
artifacts (`--dump`), the runtime trace (`--trace`), compile stats, resource
use, and the git state the run ran against. The agent then issues **SQL**,
never a rerun. The terminal output is a view over the database, never the
source of truth.

The record survives truncation because it never lived in the terminal. What
an agent would rerun with special flags to obtain, such as `--dump=lir` or
`--dump=stats`, is already in the local store.

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
elle test [paths...]            # run each path, write DB
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
  --repin                       # after the run, move the ledger to what it read (see below)
  --charge                      # run each path in-process twice; read the second run's charge on the runner heap
  --prune POLICY                # explicit history pruning (e.g. --prune adhoc)
  -N                            # stop after N failures (-1 = fail-fast); default: run to completion
```

These global `elle` flags pass through to the runner's own VM rather than
being read as corpus paths: `--trace=...`, `--boot-image=...`, and
`--dump=stats`. The I/O backend is a property of the build, so a pool-only
wedge is chased on a Linux box by building without the `uring` feature
([config](config.md)).
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
names the families once, in `WIDE_FAMILIES`, and every suite batch passes the
same list as `--wide` flags, so a family added there widens in every pass.

`--budget` answers rather than runs. It prints the budget each named path would
get, in milliseconds, one path per line, and exits zero without touching the
session store. [runner_budget.rs](../tests/integration/runner_budget.rs) reads the runner's
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

### The run's build

A reading is judged against the rows of one build, and the run's build is the
runner's own: the key the rig's `(elle/build)` answers
([rig](../rig/overview.md)). So `elle-rig test` has a build, and every child
of its `--isolate` is the same rig. A run under `elle test`, and any run with
`--host`, has none: it records no reading and judges none
([ratchet](ratchet.md)). The ledgers are `tests/ledger` under the working
directory, and a producer's path is matched as the command line gave it.

### Moving the ledger

`--repin` runs the selection as usual, then edits the ledgers under
`tests/ledger` to what the run read ([ratchet](ratchet.md)). A `stale` row
takes its new reading, and an `unledgered` reading becomes a row. A
`regression` is refused and left for a hand edit. A run with no build has
nothing to move, and `--repin` refuses it before it runs anything. The run's
rows keep the verdicts they earned, so the store says what the ledger looked
like before the move, and the exit code is the run's gate as it was. The tool
prints each row it moved.

### A file's charge

`--charge` runs each path in-process twice and reads what the second run cost
the runner's own heap, as one reading per file for the producer `elle test`
([test-gauges](test-gauges.md) says why the second run). It refuses
`--isolate` and `-e`.

### Execution and completion

By default `elle test` **runs to completion** and collects every result; it does
*not* stop at the first failure. This follows from the thesis. The value is the
**complete failure set in one shot**, fixed in one pass. A completed run leaves a
**complete DB** to query instead of rerunning. The exit code is a clean gate:
zero if and only if every selected form passed.

The agent controls the completion policy, three ways:

- **(default) run to completion** — full set, full DB, gate exit code. The
  blocking call returns once everything is recorded.
- **`-N` stop-after-N** — `-1` is fail-fast: it stops at the first failure, for
  a fast yes or no. A larger `-N` stops once N failures are recorded, for more
  than the first and less than the whole run. Scheduling stops at the Nth failure; in-flight
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
failure. The runner owns the execution order ([test-store](test-store.md)), and the
default order is **failure-likely-first**. A form that failed recently, or whose
dependency closure just changed, runs before the rest. So even a run to
completion shows the most probable failures early. The first run of a
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
[subcommand.rs](../src/program/subcommand.rs) and embedded in the binary, so a run needs no source
tree. It is built from machinery the language already exposes: the
**file-compilation pipeline** with its per-form fault-barrier mode
([test-runner](test-runner.md)), `compile/analyze`, `lib/sqlite.lisp`,
`std/compress` (zstd for the CAS), and `read-all` for ad-hoc `-e` snippets. It
adds two things of its own. One is the `gate!` gating macro. The other is the
`(compile/dumps SRC NAME)` primitive ([test-store](test-store.md)), which
returns the `--dump` artifact set as strings rather than printing it and
exiting.

The run's code state comes from `git` and `uname` through `subprocess/exec`,
and the binary's own identity from `(elle/version)`, `(elle/build-profile)`,
`(elle/executable)` and `(elle/boot-fingerprint)`. None has another source:
each is a fact about this binary, so only this binary can report it. The
executable path is what `--isolate` spawns unless `--host` names another —
a child resolved off `PATH` would be a different build, and the run would say
nothing about the one under test.
The fingerprint hashes that executable, which carries the sources it boots
from ([test-store](test-store.md)).

## Open implementation questions

- Concurrency: the suite targets get parallelism from `xargs -P` over batches
  of files. Within one run the runner joins each worker before it starts the
  next, so its forms run one at a time. Whether to run them concurrently, with
  the main thread still the only SQLite writer, is open.
- The `%assert` intrinsic's elision rules: when may the analyzer drop the
  syntax-capture (provably-true predicate, assertions-disabled build) without
  changing observable behavior for tests that *expect* a failure signal?
- Whether the gating macro `gate!` ever chooses compile-time elision over a
  runtime guard.
- Whether `:gated` is a registered signal bit or a plain user keyword. The
  answer changes the signal profile of any function that uses the loud gate.
- `form.hash` over read Syntax with comments elided is the default; confirm the
  Syntax representation actually drops comment trivia (or strip it explicitly
  before hashing).
