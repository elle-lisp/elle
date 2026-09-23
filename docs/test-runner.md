# Agent-First Test Runner

<!-- audited: 2026-09-23 -->

How a run executes: each file compiled, isolated, gated, run on every tier,
its output captured, and its end recorded honestly.

Why the runner exists and how to drive it is [test-cli](test-cli.md); where
a run is stored and what each row says is [test-store](test-store.md). The
implementation is [src/test](../src/test).

## Mechanism: the file is the unit, *compiled* — not eval'd

The runner **compiles each test file the way every real Elle program is
compiled** — Source → Reader → … → Bytecode → VM, with whole-module analysis
(binding resolution, capture analysis, signal inference across the file). It does
**not** `read-all` + `eval` form-by-form. `eval` is the REPL path (per-form
analysis, run at runtime); testing through it would exercise a path real code
never takes while *failing to exercise the file-compilation path we most need to
protect*.

With **one form per file** ([test-store](test-store.md)), `file == test == unit`, so
per-form granularity and file-path fidelity coincide for free: compile the file,
run it, record one result (× tier). Isolation is inherent — separate files,
separate module compilations, fresh scope each — and there is no cross-file
shared environment to manage; shared setup lives in imported modules, exactly as
in real code.

### Multi-form files: wrapped as one whole-file form

A file that still holds several top-level forms (legacy `tests/elle/*.lisp`, or a
multi-form `-e`) is compiled as **one module** — preserving whole-module analysis
and the file-compilation path — and **wrapped into a single whole-file thunk**:
the file's forms become the body of one `(fn () form1 form2 … formN)`, which the
runner runs once per tier as a single atomic test. This is `compile/whole-module
SOURCE NAME`. A single-form file or `-e` snippet (the durable corpus shape,
[test-store](test-store.md)) is left to the per-form path below — for one form
the two are identical.

**Why one form, not per-form.** A legacy file is an *imperative script*: it
allocates, mutates, reads back, and frees, with `def`/`var` and side-effecting
bare expressions interleaved in an order the program depends on. Running it as
one thunk
runs every form **in source order, once per tier, in isolation** — byte-for-byte
what a direct `elle FILE` run does (which is what those files were written and
verified against). Whole-module `analyze_file_letrec` still resolves bindings
across the whole file (forward references in closure bodies included; a `fn` body
is letrec-scoped exactly like a file's top level), so nothing about name
resolution changes — only execution is no longer sliced.

The earlier per-form **fault barrier** (below) hoisted every `def`/`var` to run
*eagerly* ahead of the bare-expression test forms. For an ordered script that
**reorders** the program: a `(def v (read p))` runs before the `(write p …)` that
a later bare expression performs, so `v` captures pre-write garbage; and a shared
mutable resource (an FFI pointer freed by a bare `(free p)`) is run **once per
tier** in the in-process fallback, so the second tier double-frees it. Both are
artifacts of slicing, not bugs in the test — the file passes when run directly.
Wrapping the file as one thunk eliminates the entire class.

**Atomicity is the trade.** As one form a legacy file is atomic: the **first**
failing `assert` aborts the rest, exactly as a direct run does — there is no
per-form non-abort isolation within a legacy file. That isolation was the thing
causing the reordering above; it is deliberately gone for multi-form files. A
**compile** error still fails the whole module (it always did). The durable
corpus is one-form-per-file, where “atomic” and “per-form” coincide, so this only
changes how legacy multi-form files report.

The runner does **not** compile growing prefixes (form 1, forms 1–2, …): all
forms are analyzed together, once, then wrapped.

#### The per-form barrier (single-form path / historical)

`compile/barrier-module SOURCE NAME` is the per-form mode, now used only for
single-form files. It runs the real file front-end (read → epoch extract/migrate
→ file-scope macro-expansion), then, on the **expanded** top-level forms (so
binding macros like `defn` have already become `def`), applies a transform handed
to `analyze_file_letrec`:

- `def`/`var` forms are left **unchanged** and evaluated eagerly in letrec order;
- each **test (expression) form** `E` becomes a 0-arg thunk `(fn () E)` pushed
  onto an accumulator the module returns.

`compile/whole-module` shares that front end and the same `[index thunk]` output
shape, but its transform wraps **all** forms (def/var and expressions alike) into
the body of one thunk at index 0 — so the runner's per-tier execution, the
per-tier matrix (§ Tiers), and divergence detection compose unchanged; there is
just one entry instead of N.

The catch-and-continue boundary lives **outside** the tiered closure (a worker
fiber + `protect`): a fiber-based handler *inside* a closure handed to
`compile/run-on` is rejected by the optimizing tiers (the JIT cannot create the
handler closure). So a whole-file thunk that contains a `defn` (a `MakeClosure`)
is `:ineligible` on JIT and recorded `skip` there — never silently dropped — and
runs for real on the bytecode tier.

**Boundaries (intentional).** Under `compile/whole-module` a runtime fault is the
file's single result (atomic). Under the per-form `compile/barrier-module`, a
`def` *initializer* that raises aborts the eager setup and is recorded as a single
**file-level** failure; test-form runtime failures are caught per form. With the
durable one-form-per-file corpus the module *is* the test form, so both report
identically.

### Identity and names

Each top-level form (the common case: the file's sole form) is referenced by
`file#index@line:col` and deduped across runs by a hash of its **syntax**. The
human label is scavenged from the form's syntax — the message string of its first
`assert`, falling back to leading symbols — so the author writes nothing. Within
a multi-form file the fault barrier is per top-level form; if a form holds several
`assert`s, the first to fail aborts that form, attributed precisely because the
caught `:failed-assertion` signal carries the message and span.

### Isolation: tests run in worker threads

Test code is **untrusted** — it can corrupt VM state, loop forever, or exhaust
resources. So the runner **executes each test in a worker thread with its own VM
context**, joined for its result: a fault is caught in the worker (under
`protect`) and marshalled back as a structured value, and the runner survives.
Compilation stays in the main thread — a fresh worker has no compiler context
(no symbol table) — so only *execution* is isolated. This also supplies the
internal parallelism the runner needs: workers run tests, and the main thread is
the sole SQLite writer. A *hung* test is bounded by the per-test timeout:
`os/join` takes a deadline, and a form that misses it is recorded `timeout`
instead of wedging the run. The deadline is a property of the path the form came
from, not of the run — a path the caller named wide takes the wider budget, and
every other path takes `--timeout` ([test-cli](test-cli.md)).

**Unsendable captures fall back to in-process.** A worker receives the test
thunk by deep-copying it across `os/spawn` (`SendBundle`). When the thunk
captures a value that *cannot* serialize — an FFI handle (a `db:open`
connection), a compiler artifact from `compile/*`, an arena value, a fiber, an
open file/socket port — the spawn raises a serialization `:thread-error` and the
form could never run in a worker at all. Rather than record a spurious fail, the
runner detects that specific error and **re-runs the same form in-process** in
the main VM (still under `protect` and `compile/run-on TIER`, with
`*stdout*`/`*stderr*` rebound for capture). The trade is deliberate: an
in-process form gets **no fault isolation and no timeout** (a crash or hang there
takes the runner with it), but these forms are *exactly* the ones a worker cannot
host — running them unisolated beats not running them. Sendable forms keep the
isolated, timeout-bounded worker path; only the unsendable ones degrade. (The
durable fix is per-form self-contained setup — the connection opened *inside*
each form, so it lives in the worker — which the corpus will migrate toward.)

### Isolation: a file can have its own process

A worker thread isolates a fault and shares the process. That is enough for a
form that raises, and not enough for a mode the process sets once: `--no-uring`
picks the I/O backend for the whole binary, and `--trace=guardfree` reports a
use-after-free as a SIGSEGV, which takes the runner down along with every
result it had not written yet. Those files live in
[elle_scripts.rs](../tests/integration/elle_scripts.rs) today, and their
verdicts reach no database.

`--isolate FLAGS` runs each selected path as its own child — `elle FLAGS PATH`,
one process per path — and records it on the `process` tier. The flag string is
split on spaces and may be empty.

The child's exit status is the whole verdict, because it is the whole account a
process leaves behind:

| The child | Status | Reason |
|---|---|---|
| exited 0 | `pass` | none |
| exited N | `fail` | `exit N` |
| died on a signal | `fail` | `killed by SIGSEGV (signal 11)` |
| outlived the budget | `timeout` | the budget that ran out |
| printed `SKIP (gated)` and exited 0 | `skip` | the reason the gate gave |

A signalled child is one `fail` row, and the run goes on to the next path. That
is the point of the path: a guardfree SIGSEGV kills one child and lands as a
recorded failure rather than ending the run.

`subprocess/wait` answers a signalled child with its signal number negated
([subprocess](subprocess.md)), so the sign separates an exit code from a
signal. `os/sig-name` names it, answering over the fault set as well: a child
dies on SIGSEGV, SIGABRT and SIGBUS, and the table `subprocess/kill` resolves
against refuses all three, because they are not signals a program sends
([posix-signals](posix-signals.md)).

`--timeout MS` bounds a child exactly as it bounds a worker. Two fibers drain
the child's stdout and stderr while a third waits for it, so a chatty child
cannot fill a pipe buffer and read as a hang. A child over its budget is killed
with SIGKILL, and what it printed before the kill is kept — the bargain a
timed-out worker's partial capture already makes. Both streams become `stdout`
and `stderr` assets in the CAS whatever the status.

A gated child exits 0, so its exit status alone would read as a vacuous pass —
the coverage-hiding failure the loud gate exists to prevent. The binary prints
`SKIP (gated): REASON` on that path ([main.rs](../src/main.rs)), and the runner
reads that line, so a self-gated file under `--isolate` is counted the way it
is counted everywhere else.

## Gating: a test declares where it applies

Backend- and platform-specific tests gate themselves rather than living in a
`Makefile` grep. The mechanism is a prelude macro, `(gate! COND REASON BODY…)`:
it runs `BODY` when `COND` is truthy, and otherwise raises
`{:error :gated :reason REASON}`.

```lisp
(assert (= 3 (gate! true "never gated" (+ 1 2))) "an open gate runs its body")
(let [[ok? err] (protect (gate! false "needs a GPU" :unreachable))]
  (assert (not ok?) "a shut gate raises")
  (assert (= err {:error :gated :reason "needs a GPU"}) "and names its reason"))
```

`COND` is evaluated at run time. The canonical condition is
`(backend? :jit)`: the runner compiles a test closure once and dispatches it to
every tier through `compile/run-on`, so which tier is active is known only
while the closure runs. The runner catches `:gated` and records
`status=skip, reason=REASON`. A direct `elle FILE` run prints
`SKIP (gated): REASON` and exits 0, and `--isolate` reads that line back.
Compile-time elision is not built: no silent `when!` exists, and a gate always
compiles its body.

**Gating shared setup gates the whole file.** Under the per-form barrier a
file's `def`/`var` forms run *eagerly*, once, during the barrier-module compile
to establish the shared environment — they are not per-form thunks. When an
optional dependency is acquired there (an FFI module-load that `dlopen`s
`libzmq.so`, a connection opened at top level), a `:gated` raised during that
eager phase aborts the compile *before any test thunk is built*. The runner
records this exactly parallel to a file-level compile error, but as a skip: a
single file-level row (`form_index = -1`) with `status=skip, reason=REASON`
(counted in `n_skip`; exit unaffected — a skip is not a failure). A genuine
setup error (a real exception, a syntax error in an imported library) remains
the file-level **fail**. So a file whose shared dependency is absent
self-skips *with a reason*; a file whose setup is *broken* still fails loudly.
Idiomatically the dependency is acquired through a gate at its import site —
attempt the load and re-raise a missing-library `:ffi-error` as `:gated` —
never `(sys/exit 0)`, which under the runner would terminate the whole
process mid-run and silently drop every later form.

**Why loud matters for tests.** A silently elided test looks like a form that
ran zero assertions: a vacuous pass. That is the same coverage-hiding footgun
as a tiers dial. The `:gated` error makes the skip *visible, reasoned, and
counted* in the DB, so dropped coverage is never invisible.

`elle test` needs no skip list. The per-file smoke passes in the `Makefile`
still carry `ELLE_SKIP_VM`, `ELLE_SKIP_FFI` and `WASM_SKIP`.

## Tiers are intrinsic and exhaustive — never a dial

Tier coverage is a *correctness* dimension, not a feature selector. **If we
expose `--tiers vm,jit,…` as a knob, agents will turn it down** — run
`--tiers vm`, see green, and declare victory while JIT/MLIR/WASM are broken.
That defeats the "`origin/main` is always green across every tier" invariant
([`AGENTS.md`](../AGENTS.md)). So there is no tier dial.

Every selected form runs under **every** tier, full stop. The *only* way a form
opts out of a tier is the loud gate `gate!` (§ Gating), recorded as `status=skip`
with a reason — visible, per-form, and earned, not a blanket coverage cut. The
per-tier flag soup (`--jit=off --mlir=off`, `--jit=eager`, …) and the
per-backend `Makefile` smoke targets collapse into a single exhaustive run:
`elle test`.

Cross-tier disagreement is its own status. When a form produces different values
(or different pass/fail) across tiers, the runner records `status=diverge` —
differential testing lives in the same path and the same database, never in a
separate harness ([differential](impl/differential.md)).

**Concretely (v1 representation):**

- *Tier set.* The runner attempts every candidate tier (`:bytecode`, `:jit`,
  `:wasm`, `:mlir-cpu`) but a build only carries the tiers its features were
  compiled with. A tier whose feature is absent answers `compile/run-on` with
  `:tier-rejected` / `reason :feature-disabled`; that tier is **dropped from the
  run entirely** (no row), since a feature the binary lacks is not a coverage gap
  of *this* build. The active tier set is probed once at startup and recorded in
  `run.tiers`.
- *Ineligible ≠ failed.* A tier that is present but **cannot run a particular
  form** (no LIR, a yield/`io` the JIT can't host under `compile/run-on`, …)
  answers `:tier-rejected` / `reason :ineligible`. That is recorded as a per-tier
  `status=skip` with the rejection message as the reason — visible and counted,
  never a silent drop and never a `fail`.
- *Per-tier rows stay per-tier.* Each (form × tier) still gets its own row at its
  own `pass`/`fail`/`skip` status, with `tier` ∈ {`vm`, `jit`, `wasm`,
  `mlir-cpu`} (`:bytecode` is recorded as `vm`).
- *One synthetic diverge row.* Divergence is judged over the tiers that
  **returned a value** (`status=pass`): if two or more produced *distinct* values,
  the runner appends a single extra row with `tier='*'`, `status='diverge'`, and
  `reason` rendering each tier's value (`vm=… jit=…`). The per-tier rows are left
  untouched. A divergence makes the run's gate exit non-zero (it counts in
  `run.n_diverge`), because "green on every tier" is violated.

### Concurrent runs wait, they do not collide

One path per user means every checkout shares the index, which is the point —
`--summary` and `--query` read one accumulated history, so a run must never be
pointed at a private database. Two runs therefore write to the same file.

They must queue, not fail. `lib/sqlite.lisp` opens every connection in WAL
journal mode with a busy timeout, so a writer that finds the database busy waits
for it instead of raising `sqlite-error: database is locked`. Without the wait,
the losing run dies partway through and reports `DID NOT COMPLETE — killed after
recording results for N of 25 selected files`, whose partial tally reads green
at a glance.

The timeout bounds the wait. A run blocked longer than 30 seconds still raises,
because at that point the holder is wedged rather than slow.

### Assets live in a filesystem CAS, not in the database

Storing artifact bytes as SQLite BLOBs is what makes the file balloon. Instead,
artifacts go to a content-addressed store on disk — `<db-dir>/cas/<hash>`
(compressed) — and the database stores only the hash, size, and codec. Dedup is
automatic (identical artifacts across runs and tiers are one file), the database
stays small and fast to query, and a huge artifact is just a file, not a row.

**`--trace` is the exception to "capture everything."** Trace output is too large
to retain for every form × tier. It is captured **only for forms that fail or
diverge**, written to the CAS (compressed) and referenced by hash — bounded to
exactly the cases where you'd want it, never inlined. The smaller `--dump`
artifacts are still captured for all forms, but to the CAS, not as BLOBs.

#### CAS asset capture (v1, implemented)

> **Status: `--dump` capture is OMITTED in the runner.** The
> per-file `(compile/dumps …)` pass is the single largest contributor to the
> corpus-run region leak (~28k regions/file) that OOMs `make smoke`, and the
> dumps are not byte-deterministic across compiles (absolute `@`-HirIds from a
> process-global counter), so they would not even CAS-dedup. Until the
> underlying per-compile region leak is root-caused and fixed, `capture-dumps`
> is a no-op: no `compile/dumps` call, no dump `asset` rows, no CAS dump files.
> stdout/stderr capture (below) is unaffected — it rides the per-form execution,
> not the extra dump compile. Re-enabling is a one-line revert of `capture-dumps`.

The v1 store is realized in [src/test](../src/test) plus one new compiler entry
point:

- **In-process dumps.** A new primitive `(compile/dumps SRC NAME)` compiles a
  module **once** through the real file front-end and returns a struct
  `{:ast … :fhir … :defuse … :regions … :hir … :lir … :cfg … :dfa … :jit …
  :escape …}` of the rendered artifacts as strings — the same renderings
  `elle --dump=KIND` prints, but returned in-process instead of printed-and-exit
  (the addition the *Implementation note* below calls for). It compiles the
  *unmodified* source (not the barrier-transformed module), so the dumps reflect
  the file as it really compiles. Stages that error or yield nothing are omitted.
- **The CAS.** `cas-put` content-addresses each artifact with the builtin `hash`
  (the same hash the runner uses for form identity), zstd-compresses the bytes
  (`std/compress`), and writes them to `<db-dir>/cas/<hash>` via a binary
  port — skipping the write when the file already exists (automatic dedup across
  forms, tiers, and runs). It returns `[hash size codec]`; `size` is the
  *uncompressed* length and `codec` is `"zstd"`, both recorded in the `asset`
  row. The address is over the uncompressed content, so the codec can change
  without moving the artifact.

**v1 boundaries (intentional).** Dumps are **module-level** (one compile per
file) and attached to every (form × tier) result of that file — for the durable
one-form-per-file corpus that is exact; for a legacy multi-form file each form's
result points at the whole module's artifacts. `--trace` capture, the `stats`
and `git`/SPIR-V dump kinds, and a cross-machine content hash are **deferred**:
the builtin `hash` is 64-bit and only build-stable, which is all a disposable
local cache needs (the optional CI tar-and-share is unchanged future
work). Upgrading the address to a real digest is a one-function swap in
`cas-put` once a sha256 primitive is in the core binary (the `elle-hash` plugin
is not loaded by default).

- **stdout/stderr (implemented).** Captured per (form × tier). Two facts shaped
  the mechanism: an `os/spawn` worker gets a fresh VM and serializes the *whole*
  closure into the bundle, and it has **no scheduler** — so it can do no async
  I/O (a stream write, even `port/open`, yields into the void). Both are solved
  without any per-spawn `init_stdlib`: the worker closure references stdlib's
  `ev/run`, so the serializer drags `ev/run`'s entire closure graph into the
  bundle. Sendable parameters (below) are what buy that: `*stdout*`,
  `*stderr*`, and everything `ev/run` closes over now cross the boundary. The
  worker runs the tiered call under that `ev/run` (a real scheduler), with
  `*stdout*`/`*stderr*` rebound by `parameterize` to temp files; it slurps and
  deletes them and marshals `[result stdout stderr]` back through `os/join`.
  Non-empty output becomes `stdout`/`stderr` assets on that tier's result. (A
  form that prints is I/O, so it is `:ineligible`→skip on the JIT tier, where a
  yield cannot cross `compile/run-on` — the same documented per-tier rule.)

  **A form that never returns keeps its output too.** The worker slurps and
  marshals its temp files only when the tiered call comes back, and a form
  killed by the join deadline never gets there — so `exec-thunk-capture` reads
  the partial files itself and attaches them to the `timeout` result. This is
  the case where the capture matters most: `timeout … join: deadline exceeded`
  says only that a form ran out of budget, while its output says which call it
  was in when the budget ran out. Reading the partial files also deletes them,
  so an abandoned worker leaves nothing behind in the temp root.

  The timeout's `reason` carries that last line, so the problem list reads:

  ```
  timeout  tests/elle/port-write-timeout.lisp  [vm]  join: deadline exceeded ·
      last output:     · 1: write it with :timeout 500
  ```

  A terminal-only reader — a CI log, which is the one place a wedge on a
  machine you do not have is visible — then names the call without a query. The
  whole output stays in the assets for the reader that can query.
  [timeout_capture.rs](../tests/integration/timeout_capture.rs) pins the asset,
  the reason, and the cleanup.

  **Parameters cross to the worker.** A test thunk that calls `println`
  closes over the `*stdout*` **parameter**. The serializer
  ([src/value/send](../src/value/send/mod.rs)) sends a `Parameter` when its
  default and traits are sendable, keeping its global id, since resolution is
  by id. The `Stdout`/`Stderr`/`Stdin` ports are rebuilt fresh in the worker,
  and file and socket ports stay unsendable. This is a snapshot-send,
  consistent with the per-fiber parameter snapshot.

### `assert` is a macro that carries its predicate

`assert` is a macro in [prelude.lisp](../src/prelude.lisp). On failure it raises
`:failed-assertion` with the message, the predicate's unevaluated `:syntax` and
its falsy `:value`. When the predicate is a comparison (`=`, `not=`, `<`, `>`,
`<=`, `>=`), it also records the left operand as `:actual` and the right as
`:expected`:

```lisp
(def answer 11)
(let [[ok? err] (protect (assert (= answer 10) "answer invalid"))]
  (assert (not ok?))
  (assert (= (get err :error) :failed-assertion))
  (assert (= (get err :message) "answer invalid"))
  (assert (= (get err :syntax) '(= answer 10)) "the predicate, as data")
  (assert (= (get err :actual) 11) "the left operand's value")
  (assert (= (get err :expected) 10) "the right operand's value"))
```

The runner records `:syntax` in the result row and `:message` as the form's
derived label — exact, structured, with no re-evaluation. `:actual` and
`:expected` come from the payload too, so the runner re-runs nothing. No
`%assert` intrinsic exists, so every assert runs; none is elided.

### Run honesty: a killed run must read as killed

A run that dies mid-flight — the OOM killer is the canonical case: the whole
corpus in one process can exceed the machine, and SIGKILL leaves no chance to
write anything at death — must never be readable as green-so-far. The DB records
enough at each boundary that truncation is self-evident:

- **At insert:** `n_selected` (how many files/`-e` forms the run planned) is
  written with the row.
- **Per result:** rows land incrementally (autocommit), so everything up to the
  kill survives.
- **At completion only:** the `n_*` tallies are aggregated and `finished_at` is
  stamped, in the same statement. A run row with `finished_at IS NULL` has
  therefore not reached its end: its stored counters read zero because they were
  never written, not because nothing failed.

  `finished_at IS NULL` means "did not finish", which covers *died* and *still
  running* alike — the row looks the same either way. One session DB serves
  every checkout on the machine, so a second worktree running its own corpus at
  the same time leaves in-flight rows that the first one reports as killed. The
  warning therefore names the worktree of the run it warns about, read from
  that run's `worktree` column: a path that is not yours is a sibling checkout
  still running, and a row that is still NULL once every runner has exited is
  the kill marker.

The views refuse to launder that: `--summary` and the post-run summary compute
their tallies **live** from `result` (never from the stored counters) and label
a truncated run loudly — `DID NOT COMPLETE — killed after recording results for
N of M selected files`. The next `elle test` invocation prints the same warning
about its predecessor, so a killed `make smoke` is diagnosed by the very next
run instead of reading as an all-pass mystery. Both summaries name the run's
commit, so a tally read from a log says which code it describes.
Pinned by [truncation.rs](../tests/integration/truncation.rs) and
[run_identity.rs](../tests/integration/run_identity.rs).

