# Agent-First Test Runner

> Status: **partially built** — this document is the specification, and its core
> is now implemented in [`src/test.lisp`](../src/test.lisp) as the `elle test`
> subcommand (the `smoke-elle` corpus gate). Built (v1): per-file compilation,
> the per-form fault barrier and the whole-file mode, worker-thread isolation,
> the vm/jit tier matrix with cross-tier divergence, the persistent SQLite index
> (a **subset** of the § Schema below — see the note there), the on-disk CAS for
> stdout/stderr, run honesty (a killed run reads `DID NOT COMPLETE`), `:gated`
> skips, and the `--query`/`--summary`/`--reset`/`--promote`/`-e`/`--timeout`/
> `--corpus`/`--db` flags. Still design (not built): semantic selection
> (`--touches`/`--caps`/`--impacted-by`/`--changed`/`--rerun-failed`/`-k`),
> `--rust`/`--watch`/`--prune`/`-N`/`--format`, the per-run git/RSS/CPU capture,
> `--dump`/`--trace` asset capture, `changed_file` population, and the
> predicate-carrying `assert` macro. A section marked "(v1, implemented)" /
> "(implemented)" / "**Resolved (v1)**" is built; the rest is the target.

## The problem this solves

The current Elle-script harness (`tests/elle/*.lisp` run through GNU `parallel`
from the [`Makefile`](../Makefile)) is CI-first and human-second. It is hostile
to an agent in specific, mechanical ways:

1. **The unit is the file, not the form.** `tests/elle/chan.lisp` contains ~30
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
`--stats`) are already captured in the local store.

## Architecture: the corpus is text, the database is a derived index

What feels wrong about "tests in SQLite" is real: a binary database has no
meaningful diffs, merge-conflicts badly, grows without bound, and distributes as
a blob. The fix is not a different store — it is a **one-way authority arrow**.
The database is never authoritative. Two layers:

- **The corpus is text, and lives in git.** A test is a *form*, identified by
  the hash of its syntax. Durable forms are forms in `.lisp` files, exactly as
  today — diffable, reviewable, distributed by `git clone`. This is the only
  source of truth, and it is plain text.
- **The database is a local, derived index — the history.** It lives at
  `$ELLE_CACHE/elle-tests.db`, outside the repo. It persists across many
  `elle test` invocations, accumulating run history. It holds the `form` rows
  (scanned from the corpus), the run log, results, and metadata. Authority flows
  corpus → index, never the reverse.

So git sees only text you can read; the rich queryable data is a cache that
*happens* to give you SQL joins. Distribution is `git clone` — each machine
builds its own index on first run. Sharing a *results set* (for example CI
publishing for an agent to inspect) is an optional `tar` of the index + CAS, and
it never touches the repo.

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
artifacts go to a content-addressed store on disk — `$ELLE_CACHE/elle-tests/cas/<hash>`
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

The v1 store is realized in `src/test.lisp` plus one new compiler entry point:

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
  (`std/compress`), and writes them to `<dir-of-db>/cas/<hash>` via a binary
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
  bundle (this is what made parameters sendable — below — buys: `*stdout*`,
  `*stderr*`, and everything `ev/run` closes over now cross the boundary). The
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
  `tests/integration/timeout_capture.rs` pins the asset, the reason, and the
  cleanup.

  **Prerequisite (the load-bearing part), now in the runtime.** A test thunk that
  calls `println` closes over the `*stdout*` **parameter**; `os/spawn`'s
  serializer (`src/value/send.rs`) used to reject parameters (and the stdio ports
  they default to) outright, so a printing closure couldn't even be *shipped* to
  a worker. A `Parameter` is now sendable when its default+traits are (the global
  id is preserved — resolution is by id), and the `Stdout`/`Stderr`/`Stdin` ports
  are reconstructed fresh in the worker (file/socket ports stay unsendable). This
  is a snapshot-send, consistent with the per-fiber parameter snapshot, and it
  also fixes the latent "can't spawn a printing closure" bug.

### Ad-hoc tests: born at the prompt, promotable to the corpus

An agent rarely starts with a file; it probes: `elle test -e '(assert (= (foo)
42))'`. That form runs like any other and is **persisted into the index as an
ad-hoc form** — same syntax-hash identity, tagged `origin=:adhoc` and stamped
with the session. For the rest of the session it is part of the suite: a plain
`elle test` re-runs it, `--rerun-failed` includes it, every query sees it. It is
*not* in git — it has no file.

When an ad-hoc test earns its keep, `elle test --promote <id> [file]` renders its
syntax to text and appends it to a `.lisp` file. Now it is durable: in git,
diffable, distributed, and re-derived as a durable form on the next scan. The
motion from throwaway probe to permanent regression test is one command, and
identity is preserved across it because both sides key on the same syntax hash.

Ad-hoc forms vanish when `--prune adhoc` clears them. Either way they are
ephemeral by construction, while durable forms always re-derive from the
corpus.

### The durable corpus is a flat set, classified by query

Tests are stored **one form per file** — file == test == syntax hash —
addressable, with their own blame and history, movable and promotable as atoms.
There is **no directory hierarchy**, and filesystem order is treated as
**semantically void**. This is the same argument made twice:

- *Order* is not a property of an isolated test. The runner owns execution order
  — by hash, by failure-recency, or randomized to surface hidden inter-test
  coupling — and records the order it used, so an order-dependent failure
  reproduces. Nothing about sequence belongs in a path.
- *Category* is not single-valued either. A form that does `(chan/send …)`
  inside `(fiber/new …)` while catching a signal is a channel test *and* a fiber
  test *and* a signal test. A directory forces one bucket and discards the rest;
  `compile/analyze` already yields the faceted truth (`touches`, `caps`, signal
  profile), so any grouping you'd draw as a tree is a `SELECT` over the index and
  a form appears in *every* view that applies. **Category is a query, not a
  directory.**

What promotion assigns is therefore a **name**, not a **place**: `touches
chan/send` + the derived label → `bounded-send-full.lisp`. A human *reads* the
failing test, and that legibility is the one need the index can't derive away —
order and hierarchy both can. Names are suggested from analysis and confirmed
with context at promotion.

Directories can be added later as a thin, non-authoritative reading-aid for
humans browsing the repo without the index handy — they never become the source
of classification, and the runner never depends on them.

The runner compiles any file regardless of how many forms it holds (via the
multi-form compilation mode, § Mechanism), so today's multi-form
`tests/elle/*.lisp` keep working unchanged; exploding them into the
one-form-per-file shape is a mechanical codemod for when it's convenient, not a
prerequisite.

## Non-goals / constraints (decided)

- **No new test form.** There is no `deftest`, no `check=`, no suite-metadata
  DSL — the existing `(assert cond "msg")` idiom is the whole vocabulary;
  introducing one means we missed the point. Reorganizing the corpus (one form
  per file, flat — § The durable corpus) is mechanical and preserves every form
  verbatim: it changes *where* forms live, never *how* they're written.
- **The computer names tests.** Identity is *derived*, never written. Naming is
  the runner's job, not the test author's.
- **Skip-lists die.** The few backend-specific tests vet the backend at runtime
  (§ Runtime self-vet), so the `Makefile` grep skip-lists disappear.
- **Elle drives `cargo`, not the reverse.** `cargo`'s `integration::elle_scripts`
  harness no longer drives the `.lisp` corpus — `elle test` does. What remains in
  `elle_scripts.rs` is only the few files that need a *process-global* runtime
  mode the runner cannot vary per file (`--trace=guardfree`, `--no-uring`,
  `--mlir=off`+adaptive), each run as a one-off subprocess. The remaining
  dependency to invert is the other direction: the runner *invoking* `cargo` for
  the Rust suite (`--rust`), folding its results into the same run — still future
  work.

## Mechanism: the file is the unit, *compiled* — not eval'd

The runner **compiles each test file the way every real Elle program is
compiled** — Source → Reader → … → Bytecode → VM, with whole-module analysis
(binding resolution, capture analysis, signal inference across the file). It does
**not** `read-all` + `eval` form-by-form. `eval` is the REPL path (per-form
analysis, run at runtime); testing through it would exercise a path real code
never takes while *failing to exercise the file-compilation path we most need to
protect*.

With **one form per file** (§ The durable corpus), `file == test == unit`, so
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
SOURCE NAME`. A single-form file or `-e` snippet (the durable corpus shape, § The
durable corpus) is left to the per-form path below — for one form the two are
identical.

**Why one form, not per-form.** A legacy file is an *imperative script*: it
allocates, mutates, reads back, and frees, with `def`/`var` and side-effecting
bare expressions interleaved in a load-bearing order. Running it as one thunk
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
the sole SQLite writer. Bounding a *hung* test still needs a per-test timeout,
for which there is no join-with-timeout primitive yet (Open Questions).

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
`%assert` (below).

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
separate harness (docs/impl/differential.md).

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

## What gets captured

Per **run** (one `elle test` invocation): wall time, peak RSS, user/sys CPU
(`getrusage`), the `HEAD` commit, whether the working tree is dirty, a tree hash,
the elle build version/profile/host, the full `argv`, the tier set, and the
working-tree files that differ from `HEAD` with their content hashes (the "hash
of changed files").

Per **(form × tier)**: status, reason, expected/actual and predicate syntax
(from the `assert` macro, see below), the emitted signal on failure, wall time,
and **CPU time** — the delta of `(clock/cpu)` read across the form's evaluation.

> CPU delta, not fuel. Fuel (`SIG_FUEL`) is specific to the `std/process`
> scheduler, is not consumed by Elle's default root scheduler, and essentially
> counts continuation-passing rather than CPU work — so it does not capture
> cost. A `(clock/cpu)` before/after delta does. It is not bit-for-bit
> deterministic, so regression queries on it compare distributions/thresholds,
> not exact equality; for that, prefer many runs (which we keep) over one.

Per **asset**: for every (form × tier) the runner captures the full `--dump`
artifact set (`ast, fhir, defuse, regions, escape, hir, lir, cfg, dfa, jit`),
`--stats`, and stdout/stderr — written to the filesystem CAS (§ Architecture),
deduped by hash, so identical artifacts across runs and tiers cost one file.
`--trace` is captured **only for failing/diverging forms** (too large for the
always-set), likewise to the CAS. History is **kept indefinitely**; pruning is
explicit only (`elle test --prune`), except ad-hoc forms (§ Ad-hoc tests).

> **Temporarily**, the `--dump` artifact set is **not** captured (see § CAS asset
> capture status note) — it OOMs the corpus run and does not dedup. stdout/stderr
> are still captured per (form × tier); `--dump` capture returns once the region
> leak it exposes is fixed.

> **Implementation note.** `--dump` today runs the compiler up to a stage and
> *exits*, printing to stdout. The runner must instead extract artifacts during
> the single compile+run of each form (an in-process compile option that returns
> the artifacts alongside the bytecode), so one compile+run per (form × tier)
> yields the result *and* every artifact. This is a required addition to the
> pipeline's compile entry points.

### `assert` becomes a macro that carries its predicate

This is not a new test form — it is a strict improvement to the one idiom every
test already uses. Today `assert` is a primitive that emits a bare
`{:error :failed-assertion :message …}`; the predicate's structure is lost by
the time it fails. Promoted to a **macro** in [`prelude.lisp`](../prelude.lisp)
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

## Schema

```sql
CREATE TABLE run (                  -- one row per `elle test` invocation
  id INTEGER PRIMARY KEY, started_at TEXT,
  finished_at TEXT,                 -- stamped at completion; NULL = the run was KILLED mid-flight
  git_commit TEXT, git_dirty INT, tree_hash TEXT,        -- correlate results to code state (v1: deferred)
  elle_version TEXT, build_profile TEXT, host TEXT, argv TEXT, tiers TEXT,   -- (v1: only `tiers` created)
  selection TEXT,                   -- the filter predicate; NULL = full run (the gate)
  n_selected INT,                   -- files + -e forms planned; written at insert
  n_pass INT, n_fail INT, n_skip INT, n_diverge INT, n_timeout INT,  -- aggregated at completion only
  wall_ms INT, max_rss_kb INT, cpu_user_ms INT, cpu_sys_ms INT);   -- resource usage (v1: deferred)

CREATE TABLE changed_file (         -- working tree vs HEAD at run time
  run_id INT REFERENCES run(id), path TEXT, status TEXT, blob_hash TEXT);

CREATE TABLE form (                 -- deduped across runs; the computer names it
  hash TEXT PRIMARY KEY,            -- hash of the read Syntax (comments elided)
  origin TEXT,                      -- a .lisp path (durable, in git), or ':adhoc'
  session TEXT,                     -- session id for ad-hoc forms; NULL for durable
  file TEXT, form_index INT, line INT, col INT,
  label TEXT,                       -- derived: assert message / leading symbols
  src TEXT,                         -- the form's syntax, rendered for display
  caps TEXT, touches TEXT, signal TEXT);   -- from compile/analyze — drives selection

CREATE TABLE result (               -- one row per (form × tier × run)
  id INTEGER PRIMARY KEY, run_id INT REFERENCES run(id),
  form_hash TEXT REFERENCES form(hash), tier TEXT,
  status TEXT,                      -- pass|fail|skip|diverge|error
  reason TEXT, expected TEXT, actual TEXT, syntax TEXT, signal TEXT,
  wall_ms INT, cpu_us INT);       -- cpu_us = (clock/cpu) delta across the form

CREATE TABLE asset (                -- artifact attached to a result; bytes live in the CAS
  result_id INT REFERENCES result(id),
  kind TEXT,                        -- ast|fhir|hir|lir|cfg|dfa|jit|stats|stdout|stderr|trace
  hash TEXT, size INT, codec TEXT); -- bytes at $ELLE_CACHE/elle-tests/cas/<hash>; codec e.g. zstd
```

The runner writes this with `lib/sqlite.lisp` (FFI to libsqlite3). The DB holds
only metadata and hashes; artifact bytes live in the on-disk CAS, so the file
stays small and merge/diff concerns never arise (it is gitignored regardless).

**v1 implemented subset (`src/test.lisp` `ensure-schema`).** The runner creates
`form`, `result`, and `asset` with the columns above; `run` and `changed_file`
are subsets:

- `run` is exactly `id, started_at, finished_at, tiers, selection, n_selected,
  n_pass, n_fail, n_skip, n_diverge, n_timeout` — the code-state
  (`git_commit`/`git_dirty`/`tree_hash`/`elle_version`/`build_profile`/`host`/
  `argv`) and resource (`wall_ms`/`max_rss_kb`/`cpu_user_ms`/`cpu_sys_ms`)
  columns are deferred. So the regression-archaeology query below
  (`min(run.git_commit)`) and any resource query are design-only until those
  columns land; a `SELECT` of a deferred column errors with `no such column`.
- `changed_file` is created but never populated (no `--changed`/git capture yet).

## The agent workflow, as SQL

Everything below is one query against `$ELLE_CACHE/elle-tests.db`. None of it
re-runs the suite.

```sql
-- What failed, completely, in the latest run — full detail, immune to truncation.
SELECT f.file, f.line, f.label, r.tier, r.expected, r.actual, r.signal
FROM result r JOIN form f ON f.hash = r.form_hash
WHERE r.run_id = (SELECT max(id) FROM run) AND r.status = 'fail';

-- Locate the LIR for a failing form WITHOUT re-running `elle --dump=lir`.
-- Returns the CAS hash; read the (compressed) bytes from $ELLE_CACHE/elle-tests/cas/<hash>.
SELECT hash, codec FROM asset WHERE result_id = ? AND kind = 'lir';

-- Regression archaeology: when did this form first start failing?
SELECT min(run.git_commit) FROM result JOIN run ON run.id = result.run_id
WHERE result.form_hash = ? AND result.status = 'fail';

-- Perf drift: forms whose CPU time rose materially vs a baseline run.
SELECT cur.form_hash, cur.tier, base.cpu_us AS was, cur.cpu_us AS now
FROM result cur JOIN result base
  ON base.form_hash = cur.form_hash AND base.tier = cur.tier
WHERE cur.run_id = ? AND base.run_id = ? AND cur.cpu_us > base.cpu_us * 2;
```

## Selection (uses `compile/analyze`)

Selection narrows *which forms* run; it never narrows *which tiers* (§ Tiers are
intrinsic). These are **inner-loop accelerators**, not the gate. Any filtered run
records its predicate in `run.selection`, so a partial run is visibly partial and
cannot be passed off as a full green. The gate — what CI, the merge queue, and a
"done" claim require — is a `selection IS NULL` run: every form, every tier.

Because the runner holds forms as data and analyzes them before eval, selection
is semantic, not just glob/name:

| Flag | Selects |
|------|---------|
| `tests/elle/chan.lisp` | a file (positional) |
| `-k SUBSTR` | forms whose derived label matches |
| `--touches chan/send` | forms whose analysis references that binding |
| `--caps io` | forms that exercise an I/O capability |
| `--impacted-by <ref>` | forms whose dependency closure intersects the diff |
| `--changed` | forms whose source + dependency closure are untouched since their last green result are skipped (fast inner loop) |
| `--rerun-failed` | the failing set from the last run (read from the DB) |

`--changed` is the incremental payoff of the datastore: `form.hash` plus
`caps`/`touches` from `compile/analyze` plus the run's `changed_file` set tell
the runner exactly which forms can be safely skipped. Full run stays the default.

## CLI surface

```
elle test [paths...]            # default: tests/elle, ALL tiers, write DB
                                # (no --tiers flag — tier coverage is not a dial)
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
  --prune POLICY                # explicit history pruning (e.g. --prune adhoc)
  -N                            # stop after N failures (-1 = fail-fast); default: run to completion
```

### Execution and completion

By default `elle test` **runs to completion** and collects every result — it does
*not* stop at the first failure (the opposite of today's `--halt now,fail=1`).
This follows from the thesis: the value is the **complete failure set in one
shot** (fix everything in one pass, not fix-one-rerun-see-next), a completed run
leaves a **complete DB** (query instead of re-running), and the exit code is a
clean gate — zero iff every selected form passed on every tier.

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

The key point your completion question exposes: once results are persisted
incrementally, "first actionable event" and "run to completion" stop competing.
The suite finishes (the DB fills) while the agent is already working the earliest
failure. And because the runner owns execution order (§ The durable corpus), the
default order is **failure-likely-first** — forms that failed recently or whose
dependency closure just changed run before the rest — so even a run-to-completion
pass surfaces the most probable failures early in wall-clock. The first run of a
fresh branch has no history to order by, so it simply runs everything in scan
order; the prioritization kicks in once results exist.

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
  stamped, in the same statement. A run row with `finished_at IS NULL` therefore
  *is* the kill marker: its stored counters read zero because they were never
  written, not because nothing failed.

The views refuse to launder that: `--summary` and the post-run summary compute
their tallies **live** from `result` (never from the stored counters) and label
a truncated run loudly — `DID NOT COMPLETE — killed after recording results for
N of M selected files`. The next `elle test` invocation prints the same warning
about its predecessor, so a killed `make smoke` is diagnosed by the very next
run instead of reading as an all-pass mystery.
Pinned by `tests/integration/truncation.rs`.

### `--rust`: folding in the cargo suite

`elle test --rust` invokes `cargo test --message-format=json`, parses libtest's
JSON, and writes one `result` row per Rust test with `tier='rust'`, capturing
cargo's stdout/stderr as assets under the same `run`. The whole suite — Elle
behavioral, differential, and Rust — lives in one queryable transaction. (If the
libtest JSON format turns out to be unstable on the toolchain in use, fall back
to parsing the human output — but it's expected to hold.)

## Substrate

The runner is an **Elle program**, living at **`src/test.lisp`** and surfaced as
the `elle test` subcommand alongside `elle fmt`/`elle lint` (it may grow into a
`src/test/` directory if it outgrows one file). It is built from machinery the
language already exposes — the **file-compilation pipeline** (plus the per-form
fault-barrier compilation mode, § Mechanism), `compile/analyze`, the tier
backends, `lib/sqlite.lisp`, `std/compress` (zstd for the CAS), and `read-all`
for ad-hoc `-e` snippets — plus the additions noted above: the
`when!`/`unless!`/`gate!` gating macros (general-purpose conditional
compilation) with their compile-time predicates (`backend?`, `feature?`, …),
and the in-process artifact-capture compile option, realized as the
`(compile/dumps SRC NAME)` primitive (§ CAS asset capture) that returns the
`--dump` artifact set as strings rather than printing them and exiting.

## Open implementation questions (for the tests/code phases)

- `(clock/cpu)` granularity and whether per-form deltas are meaningful under the
  faster tiers (a JIT'd form may run in sub-microsecond territory). Decide
  per-tier whether to record CPU per-form, per-file, or only run-level.
- Concurrency: the current harness gets parallelism from GNU `parallel` across
  files. The new runner parallelizes across forms/files internally (worker
  threads, § Isolation) while keeping SQLite writes serialized (single writer,
  WAL).
- Per-test **timeout**: there is no `os/join`-with-timeout (or thread-cancel)
  primitive today, so a hung test blocks its join. Needs either such a primitive
  or a channel/`chan/select`-with-timeout pattern (and a way to abandon the
  worker), so a runaway test is recorded as `timeout` rather than wedging the run.
- The `%assert` intrinsic's elision rules: when may the analyzer drop the
  syntax-capture (provably-true predicate, assertions-disabled build) without
  changing observable behavior for tests that *expect* a failure signal?
- The gating macros (`when!`/`unless!`/`gate!`): how `(backend? …)` etc. are
  exposed as compile-time constants under forced tiers, how `gate!` chooses
  compile-time elision vs runtime-guard lowering, and whether `:gated` is a
  registered signal bit or a plain user keyword (it changes the signal profile of
  any function using the loud gate).
- `form.hash` over read Syntax with comments elided is the default; confirm the
  Syntax representation actually drops comment trivia (or strip it explicitly
  before hashing).
- The per-form fault-barrier compilation mode: how a top-level form's signal is
  caught and the next form resumed *within one compiled module* without nesting
  forms in lambdas (which would break top-level binding scope). **Resolved (v1):**
  the file is compiled once through `analyze_file_letrec`; `def`/`var` forms run
  eagerly to establish shared bindings while each test form is reified as a thunk
  capturing that environment; the runner runs each thunk per tier with the fault
  barrier *outside* the tiered closure. The catch-and-continue is therefore a
  bytecode-tier property (the optimizing tiers reject in-closure handlers and any
  signal that crosses `compile/run-on`). See § Mechanism → "How the barrier is
  realized (v1)" for the full mechanism and its intentional boundaries. A future
  iteration could push per-form catching into the optimizing tiers via a genuine
  instruction-level handler region (none exists in the bytecode today).
- Divergence sampling vs full matrix. Full matrix is the gate; the open question
  is whether the inner loop may run the canonical tier on everything and *sample*
  the others for divergence — permitted only if coverage is recorded (it
  accumulates across runs) and the run is marked partial in `run.selection`, so
  it is never a silent coverage cut.
```
