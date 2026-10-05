# The ratchet

<!-- audited: 2026-10-05 -->

One library measures, one committed ledger holds every bound, and the runner
judges, records and re-pins; nothing else carries a number.

This document is the specification. The instrument and the ledger are built,
as [the guide](../lib/ratchet.md) shows, and so is the runner's side: the
judge, the rows, the `missing` gate, the summary and `--repin`. A producer is
on the ratchet when `tests/ledger` holds a file for it. The producers under
§ The producers are proposed.

## What a ratchet is

A ratchet is a test that pins a measured number and lets it move one way. The
leak dashboards pin a rate per operation and let it fall. A residue test pins a
ceiling on the objects a request leaves behind. A canary pins the allocations a
loop makes. Each pin is the reading the tree gave on the day somebody accepted
it, and a change that moves the reading the wrong way fails.

## What the copies get wrong

A ratchet written by hand is a pin as a literal in the test, a window helper
beside it, and a "shrink-only" comment over both. Every such copy gets the
same four things wrong.

**The bound is prose.** A pin is a literal in a source file, and "shrink-only"
is a comment beside it. Nothing fails when a pin is raised. Lowering one is a
hand edit, so a fix that reclaims more than expected leaves a loose pin behind,
and a later regression back to the old number passes.

**Coverage is a convention.** Which subjects are read on which axes is decided
by whoever writes the subject, and recorded nowhere but the source. Issue
elle-lisp/elle#1144 asks the question nothing can answer: which subject has no
rate?

**Every copy rebuilds the same machinery.** The gauge-live discriminator, the
window, the ceiling and the failure message are each written again per test.
A residue test writes the smallest copy, with no reading line at all, so its
ceiling has no history.

**A tolerance on the pin hides a leak.** A pin matched to within half a unit
passes a shape pinned at zero at 0.3 objects per operation, which is the exact
rate an integer slope hid. The tolerance a pin needs is the reading's own
uncertainty, and nothing else.

## The design

A measurement is a fact. A bound is a claim. The two never live in one file.

| Part | Where | What it owns |
|------|-------|--------------|
| The instrument | `lib/ratchet.lisp` and `lib/ratchet/estimator.lisp` | gauges, the estimator, the drives, the reading line |
| The reading | one line on stdout | subject, axis, value, unit, uncertainty |
| The ledger | `tests/ledger/*.lisp`, committed | one row per (subject, axis) and build: the bound, its kind, its class |
| The ledger module | `lib/ratchet/ledger.lisp` and `lib/ratchet/repin.lisp` | the row reader, the judge, the line reader, the re-pin's rewrite |
| The runner | `src/test/ledger.lisp` and `src/test/repin.lisp` | reading every line back, judging it, the completeness gate, history, `--repin` |

A producer imports the instrument, drives its shapes, and prints one line per
reading. It judges nothing. Under `elle-rig test` the runner reads the lines
back, judges each against the rows of its own build, adds the rows no producer
answered, records every reading, and offers to move the ledger to what was
read.

### The reading line

Every reading is one line on stdout, opened by the word `measure` and carrying
one JSON object:

```text
measure {"subject":"io-drop","axis":"objects","value":0.0,"half":0.04,"unit":"objects/op"}
```

`subject` names the shape, `axis` the dimension, `value` the reading and
`unit` what one unit of it is. `half` is the half-width of the reading's
interval: what the estimator reports for a rate, and 0 for an exact count. The
instrument adds `void` to a reading it refuses, `class` and `floor` to a
live-growth reading, and `alt-value` to a rate it read at two block sizes. A
line never carries a bound, a kind or a verdict: those are the runner's.

The line is the rendering. There is no second, prettier copy for a human, so a
direct run and a captured asset show the same text. Stdout is the channel
because everything that measures can print: an Elle program in a worker
thread, an isolated child, a Rust test, a shell script driving `valgrind`. The
runner already captures stdout for every form, every tier and every child, so
no environment variable, file or process boundary is needed. This is the
mechanism `SKIP (gated):` uses today ([test-runner](test-runner.md)).

### The ledger

The ledger is a directory of Elle data files, `tests/ledger/`, one file per
producer, each under the reading budget. A file opens with the epoch
declaration every Elle file carries, then the producer it answers for as
`(producer "tests/impl/plumb.lisp")`, then one row per line:

```text
["objects gauge (live-growth)" :objects :floor 0.5 :class :growth]
["regions gauge (live-growth)" :regions :floor 0.5 :class :growth]
["io-yield ev/sleep" :objects 0]
["io-drop" :objects 0]
["io-drop" :regions 0]
["subprocess-exec" :objects 0 :note "a block here is a block of child processes"]
```

A row is `[subject axis bound & options]`. The bound is one of three kinds:

| Written | Kind | Passes when |
|---------|------|-------------|
| `0`, `1.002` | pin | the reading's interval overlaps the pin, within the slack |
| `:floor 0.5` | floor | the interval's top is at or above the floor |
| `:ceiling 8` | ceiling | the interval's bottom is at or below the ceiling |

A pin is the last accepted reading, and it is two-sided. A reading past it on
the worse side is a `regression`. A reading past it on the better side is
`stale`: the pin has slipped behind the tree, and a later regression back to
it would pass. Both fail the gate. A floor and a ceiling are thresholds rather
than readings, so neither is ever stale. The discriminators, the sub-integer
self-test and a control-ratio bound are thresholds; every other row is a pin.

The options:

| Option | Meaning |
|--------|---------|
| `:class :growth` | the shape must grow by design; the row is a floor, and a floor that fails voids every other row on its axis from this producer |
| `:class :defect` | an open leak, under the root `:root` names; these rows are the burndown |
| `:root :f1a` | the leak class the row answers to: the one an open defect is under, or the one a regression of a control would reopen |
| `:better :higher` | a larger reading is the better side; the default is `:lower` |
| `:slack N` | the reading may sit this far past the pin on either side before it is judged; the default is 0 |
| `:note "…"` | one sentence for the reader, kept when the tool rewrites the row |
| `:build "…"` | the build the row belongs to; a row without one belongs to the reference build |

A row belongs to exactly one build. The reference build is the default build
on Linux x86_64, the one the `Default Build Tests` job runs, and its key is
`jit-uring-linux-x86_64`. A row with no `:build` belongs to it, and a `:build`
naming that key means the same. A reading is judged against the rows of its
own build alone, and every pin is two-sided there. A row of another build is
no row at all.

A row with no class is a control: a shape the tree reclaims, pinned at what it
reads. The slack is for a subject the machine makes noisy, a wall-clock ratio
for instance. It is not for the estimator's uncertainty, which every rate
already carries as `half`. Nor is it a resolution. A rate's resolution is the
epsilon the producer measures it to, and a producer that wants to see a tenth
of an object per operation measures to a tenth.

Comments sit on lines of their own. The tool that moves a bound rewrites the
bound's token inside the row's brackets and touches nothing else, so a comment
survives every re-pin, and so does the wrapping `elle fmt` gives a long row.
`elle fmt` formats a ledger file like any other.

### The build

The rig names the build, and computes its key from its own features:
`(elle/build)` answers the tier, the I/O backend, the operating system and the
architecture, joined by `-` ([rig](../rig/overview.md)). No primitive of a user
build names it. So `mlir-uring-linux-x86_64` and `jit-pool-macos-aarch64` are
two builds beside the reference build. A profile or a sidecar changes how a
build runs a file and never which build it is, so the eager pass of the
implementation suite judges against the same rows as the plain pass.

### The judge

Every reading meets its row, and the outcome is one of six verdicts:

| Verdict | Meaning | Gates |
|---------|---------|-------|
| `ok` | the reading is within its bound | no |
| `regression` | the reading moved the worse way | yes |
| `stale` | the reading moved the better way past the slack; re-pin it | yes |
| `unledgered` | a reading with no row; adopt it or delete the measurement | yes |
| `missing` | a row whose producer ran and reported nothing | yes |
| `void` | the instrument's own check failed, so the reading says nothing | yes |

The comparison is on intervals. Take a reading `v ± h` against a pin `p` with
slack `s`, under `:better :lower`. It is a regression when `v - h > p + s`,
and stale when `v + h < p - s`. The sides swap under `:better :higher`. A
floor fails when `v + h` is below it and a ceiling when `v - h` is above it. So a rate
measured to a wide epsilon passes a pin it straddles, and only a rate measured
tightly enough to clear the pin can fail it. What the instrument can see is
what the gate can hold.

A void reading is one the instrument refuses to stand behind. That is a rate
whose B-invariance check found it block-dependent, a growth row that read
flat, or any later reading on an axis whose growth row read flat. The message
names the cause and the rows it voids, once, here. Today each dashboard writes
that sentence itself.

`missing` is what closes elle-lisp/elle#1144. The ledger names every subject
and axis the tree claims to measure, and a run that reports none of a claimed
row fails on it. Coverage cannot rot into silence in either direction: a
deleted probe leaves a `missing` row, and a new probe leaves an `unledgered`
reading, until somebody moves the ledger.

### The runner

The runner reads every `measure` line out of every captured stdout. What it
does with a reading turns on the run's build, which is the runner's own: the
key `(elle/build)` answers under `elle-rig test`, and none under `elle test`.
A child of `--isolate` with no `--host` runs `(elle/executable)`, the same
rig, so the key holds for every child. A run with `--host` runs its children
on another program, and has no build.

| The run | Its readings |
|---------|--------------|
| no build: `elle test`, or any run with `--host` | neither recorded nor judged: no `missing` row, no gate, and `--repin` refuses |
| a build, and no ledger file names the producer | recorded with no verdict: no `missing` row, no gate |
| a build, and the producer's ledger holds no row of that build | recorded with no verdict, and `--repin` adopts every one as a row of that build |
| a build, and the ledger holds rows of that build | judged against those rows and recorded, and a row nobody read is `missing` |

The ledger directory is `tests/ledger` under the working directory, and a
producer's path is matched relative to the working directory, as the runner
takes every path.

A producer runs in-process on the rig, in a pass of its own, under both JIT
policies, and has no sidecar ([test-runner](test-runner.md)). The runner wraps
a file of several forms as one whole-file form, so a producer's top-level
definitions are locals of one function. A probe is written for that shape: it
reads the same as a local as it does at the top level of a direct run. A judged or unjudged reading is one `measurement` row:

```sql
CREATE TABLE measurement (
  run_id INT REFERENCES run(id),
  result_id INT REFERENCES result(id),   -- the form × tier, or the child, that printed it
  subject TEXT, axis TEXT,
  value REAL, half REAL, unit TEXT,
  bound REAL, kind TEXT,                 -- the row it met: pin|floor|ceiling, or NULL when unledgered
  verdict TEXT);                         -- ok|regression|stale|unledgered|missing|void
```

After each result lands as a `pass` the runner walks the rows its build holds
in that file's ledger. A row the result printed no reading for becomes a
`missing` row against it. A producer that gated itself out has skipped, not
failed to report, and its rows are left alone. A run with a selection can only
hold the producers it ran, so the full gate is a `selection IS NULL` run, as
it is for every other verdict.

The summary counts readings by verdict and lists every one that is not `ok`:

```text
412 readings · 409 ok · 1 regression · 1 stale · 1 missing
  regression  tests/impl/oracle.lisp  [process]  reduce  objects  1.31 ±0.12 objects/op  pinned 1.002
  stale  tests/impl/plumb.lisp  [process]  ev-abort  regions  0.0 ±0.03 regions/op  pinned 1
  missing  tests/impl/oracle.lisp  [process]  fiber-nested  regions
```

The listing reads in the tally's order and names the tier, because a form
runs once per tier and each run's reading is a row of its own. The gate fails
on any verdict but `ok`, exactly as it fails on a form. A run with no build
whose results printed a reading says, in one line, that it recorded and judged
none of them, so a green run does not read as a passed gate.

`elle-rig test --repin PATHS` runs the selection, then moves the ledger to what
it read: every `stale` row takes the new reading, and every `unledgered`
reading becomes a row. It refuses a `regression`. A bound moves the worse way
by a hand edit, which the pull request's diff then shows beside the change that
needed it. The tool prints each row it moved. A reading it re-pins is written
to three significant figures for a rate and as the integer it is for a count.

A subject read on several tiers moves to the worst of its readings, so the
new pin holds on every tier. The tool moves rows of the run's build alone. An
`unledgered` reading of a growth class, which is the instrument's own
live-growth row, is adopted as a growth floor at the floor the instrument
named; every other `unledgered` reading is adopted as a pin at its value. A
build whose ledger holds no row of its own has every reading adopted, which is
how a build joins the ratchet. A reading adopted on a build other than the
reference build is adopted as a `:build` row. A producer with no ledger file
gets one by hand, never from the tool. The rewrite is in
[repin.lisp](../lib/ratchet/repin.lisp): it scans the ledger's text for the
row's brackets, replaces the bound's token, and appends an adopted row after
the last one.

A `run` row records the build's key, or NULL for a run with none, so a
reading's history groups by the build that read it ([test-store](test-store.md)).
The runner imports the ledger module for the row reader, the judge and the line
reader, and the re-pin module for the rewrite.

The runner is also a producer. It reads its own three gauges at every file
boundary today and pins none of them. At the end of a run it reports, for the
producer `elle test`, the largest delta any one file charged on each gauge.
The ledger pins the three. A compiler change that raises every file's compile
cost moves the maximum, which is the regression this exists to catch, and the
per-file rows in `gauge` keep saying which file.

### The instrument

`(import "std/ratchet")` answers a closure; calling it makes one instrument
with its own readings. The instrument measures and prints. It reads no ledger,
and it needs neither its producer's path nor its build. A direct run of a
producer prints the same lines the runner reads, and judges none of them.

| Export | What it does |
|--------|--------------|
| `objects`, `regions`, `bytes`, `pages`, `ids` | the arena gauges, each with its axis and unit |
| `(gauge axis unit read)` | a gauge of the caller's own: the operand-stack depth of elle-lisp/elle#1135 when it lands |
| `(rate subject probe & opts)` | the adaptive per-op rate of `(probe j)`, reported on `:on` gauges (default `objects`), to `:epsilon`, over `:block` ops per block between `:min` and `:max` blocks; `:stable true` runs the B-invariance check |
| `(drive subject run-block & opts)` | the same rate over a run-block of the caller's own, `(fn [b])` performing `b` ops: a tail recursion, a fiber drained after `b` yields, a loop that must be a discarded statement |
| `(stmt-run thunk)` | the run-block that calls `thunk` `b` times as a discarded statement, the while-loop shape a per-call leak needs to show |
| `(delta subject gauge body :n N)` | the gauge's change over `N` runs of `body`, per run, after one uncounted run |
| `(ratio subject measured control)` | the smaller of several alternating timings of `measured` over the same of `control` |
| `(read subject axis value & opts)` | any number from anywhere: a count a script parsed, a byte total `valgrind` printed |

**The instrument proves each gauge live.** The first time a producer reads an
axis, the instrument drives that gauge's own live-growth shape first and
reports it as `<axis> gauge (live-growth)`. Its row is a growth floor, and the
runner voids the axis when that floor fails. No producer writes a
discriminator, and none can forget one. A module-level sink keeps every struct
it is handed, so the object count, the region count and the byte count climb.
A sink of pairs keeps the free list drained, so the id counter climbs.

## The producers

The producers proposed here are the ones the design has to specify because
nothing else does: each reads a number no test reads today. A test that
already measures joins the ratchet by importing the instrument and committing
its ledger, and needs no paragraph here; what it reads is that test's header,
and why its rows are what they are is its ledger's comments.

**The runner**, as above.

**valgrind.** A producer, `tests/ratchet/valgrind.lisp`, gates itself on the
binary being present, runs a fixed set of programs under `memcheck`, parses
the leak summary, and reads the definitely-lost bytes and the error count per
program. Each is a pin at 0 or at what the tree leaks today. The producer runs
as an isolated child under its own budget, because a run under `memcheck`
costs minutes.

**Rust tests** can print the same line, and nothing collects it until
`elle test --rust` exists ([test-cli](test-cli.md)). The line format is the
whole contract, so that step adds a parser and no second channel.

## Landing order

Each phase lands as documentation, then a failing test, then code, and each
is one pull request. The first four are in.

1. **The library and the ledger.** `lib/ratchet.lisp` with `rate`, `delta`,
   `read` and the gauges, the judge and the row reader, and `tests/ledger/`.
   The counter-factual: the judge, as a pure function, calls a reading past
   its pin a regression and one past it the better way stale.
2. **The runner reads the line.** `src/test/ledger.lisp` parses stdout for
   every form and child, writes the rows, adds `missing`, prints the summary
   and gates. The counter-factual: a ledger row no producer answers fails the
   run.
3. **`--repin`.** The counter-factual: a stale row is rewritten, a regression
   is refused, and the comment above the row survives.
4. **The dashboards move.** Oracle and plumb over the library, their pins in
   two ledger files, and their headers shortened to what the library does not
   already say. The counter-factual is the dashboards' own: every row reads
   what it read before the move.
5. **The residue tests, the runner's own gauges, the audit queue, valgrind.**
   One producer per pull request, each with its ledger. A residue test
   replaces its window helper with `rate` or `delta`, its ceiling with a row,
   and its gauge-live gate with the instrument's live-growth row; a gauge it
   kept for its own sake is a growth floor. The counter-factual is the ledger
   itself: committed ahead of the producer, every row of it is `missing` until
   the producer reads it.

## Decisions this document takes

- A stale pin fails the gate. The alternative is a warning, which is what a
  comment saying shrink-only is today.
- The runner is the only judge, and the instrument measures and prints. One
  judge is one code path, and the producer's path and the ledger lookup stay
  out of programs. A direct run's verdict is not a requirement. The instrument
  could judge every verdict but `stale` without knowing its build, so the
  reason is economy.
- A row belongs to exactly one build, and every pin is two-sided there. A
  build is an implementation, and one implementation's footprint says nothing
  about another's: a build may trade a leak in one area for a gain in another.
  Nothing is one-sided anywhere.
- A run with no build records and judges nothing. A reading is judged against
  its build's rows, and a run with no build has none to judge against.
- The main runtime offers no primitive that names the build; `(elle/build)`
  exists in `elle-rig` alone. Test infrastructure adds no runtime surface for
  its own convenience. A program can still infer parts of its build:
  `(vm/config :jit)` answers the tier, and `uname` the platform.
- Readings travel on stdout. The alternative keeps the environment-variable
  file, which no worker thread and no foreign producer can use.
- The bound lives in the ledger, never in the producer. The alternative keeps
  numbers in source and adds a tool that edits source, which the rewrite engine
  can do; it leaves coverage a convention and the raise unenforced.
