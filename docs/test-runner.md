# Agent-First Test Runner

<!-- audited: 2026-09-17 -->

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
instead of wedging the run.

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

## Gating: compile-time enable/disable (replaces skip-lists)

Backend- and platform-specific tests should not live in a `Makefile` grep — they
gate themselves. The right tool is general, not test-specific: a **compile-time
conditional-compilation macro** usable anywhere in the language (Elle's `#[cfg]`),
in two variants. Bang marks compile-time, per convention.

**Silent — `(when! COND BODY…)` / `(unless! COND BODY…)`.** `COND` resolves at
expansion/analysis time against compile-time facts: the active tier
(`(backend? :jit)` under the runner's forced policies), features
(`(feature? :ffi)`), OS, epoch. When it excludes the block, the block is *not
compiled* — no runtime cost, no dead code, and the excluded text may even
reference bindings that don't exist in the other configuration. General-purpose;
no tests involved.

**Loud — `(gate! COND REASON BODY…)`.** The same gate, but an unmet `COND` does
not vanish silently — the site emits `(emit :gated {:reason REASON})`, a signal a
harness can catch and account for. This is what tests use:

```
(gate! (backend? :jit) "needs JIT" <body>)          # compile-time: the VM tier emits :gated
(gate! (ffi-available? "libsqlite3.so")             # runtime condition: lowers to a
       "libsqlite3 not installed" <body>)           #   runtime guard that emits :gated
```

When `COND` is compile-time-constant the macro decides at compile time (dead
branch uncompiled); when it isn't (library presence, `$DISPLAY`, …) it lowers to
a runtime guard that emits `:gated` on the unmet path. Either way the runner
catches `:gated` and records `status=skip, reason=REASON`.

**Gating shared setup gates the whole file.** A file's `def`/`var` forms run
*eagerly*, once, during the barrier-module compile to establish the shared
environment — they are not per-form thunks. When an optional dependency is
acquired there (an FFI module-load that `dlopen`s `libzmq.so`, a connection
opened at top level), a `:gated` raised during that eager phase aborts the
compile *before any test thunk is built* — the test forms never become runnable
units. The runner records this exactly parallel to a file-level compile error,
but as a skip rather than a fail: a single file-level row (`form_index = -1`)
with `status=skip, reason=REASON` (counted in `n_skip`; exit unaffected — a skip
is not a failure). This is distinct from a genuine setup error (a real exception,
a syntax error in an imported library), which remains the file-level **fail**. So
a file whose shared dependency is absent self-skips *with a reason*; a file whose
setup is *broken* still fails loudly. Idiomatically the dependency is acquired
through a gate at its import site — attempt the load and re-raise a
missing-library `:ffi-error` as `:gated` — never `(sys/exit 0)`, which under the
runner would terminate the whole process mid-run and silently drop every later
form.

**Why loud matters for tests — and silent is wrong for them.** A silently elided
test looks like a form that ran zero assertions: a vacuous pass. That is the same
coverage-hiding footgun as a tiers dial. The `:gated` signal makes the skip
*visible, reasoned, and counted* in the DB, so dropped coverage is never
invisible. Code that genuinely wants a block to disappear uses silent `when!`;
anything whose absence must be accounted for uses `gate!`.

This is one general mechanism, not two test primitives (it subsumes the earlier
`skip-if`/`skip-unless`), and it deletes the `ELLE_SKIP_VM` / `WASM_SKIP` /
`ELLE_SKIP_FFI` Makefile lists: each test declares its own applicability where it
lives, introspectably. It shares the compile-time gating/elision machinery with
`%assert` (§ `assert` becomes a macro).

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

  **The prerequisite this rests on, now in the runtime.** A test thunk that
  calls `println` closes over the `*stdout*` **parameter**; `os/spawn`'s
  serializer (`src/value/send.rs`) used to reject parameters (and the stdio ports
  they default to) outright, so a printing closure couldn't even be *shipped* to
  a worker. A `Parameter` is now sendable when its default+traits are (the global
  id is preserved — resolution is by id), and the `Stdout`/`Stderr`/`Stdin` ports
  are reconstructed fresh in the worker (file/socket ports stay unsendable). This
  is a snapshot-send, consistent with the per-fiber parameter snapshot, and it
  also fixes the latent "can't spawn a printing closure" bug.

### `assert` becomes a macro that carries its predicate

This is not a new test form — it is a strict improvement to the one idiom every
test already uses. Today `assert` is a primitive that emits a bare
`{:error :failed-assertion :message …}`; the predicate's structure is lost by
the time it fails. Promoted to a **macro** in [prelude.lisp](../src/prelude.lisp)
alongside the other macros, `assert` captures the predicate's syntax and result
into the payload (the error keyword stays `:failed-assertion`):

```
(assert (= x 10) "x invalid")
# on failure emits:
(error {:error   :failed-assertion
        :message "x invalid"
        :value   false            # the predicate's evaluated result
        :syntax  '(= x 10)})      # the predicate, unevaluated, as data
```

The runner records `:syntax` in the result row and `:message` as the form's
derived label — exact, structured, free, with no re-evaluation guesswork.
(`:value` is always `false` for a failed assert, so it is not a column.) When
`:syntax` is a recognized comparison (`(= a b)`, `(< a b)`, …), the macro
additionally embeds each operand's value so `actual` (the LHS) and `expected`
(the RHS) are populated without the runner re-running anything. Existing `(assert cond "msg")` call
sites are untouched; they just start failing *informatively*.

The macro likely bottoms out in an `%assert` intrinsic carrying the captured
syntax, so the analyzer can recognize it and **elide it in circumstances where
it provably cannot fail** (or where assertions are disabled for a build),
keeping the syntax-capture from costing anything at runtime when it isn't needed.

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

