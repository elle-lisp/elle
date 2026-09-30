# The ratchet

<!-- audited: 2026-09-30 -->

One library measures, one committed ledger holds every bound, and `elle test`
judges, records and re-pins; nothing else carries a number.

This document is the specification. The instrument, the ledger and the
direct-run gate are built, as [the guide](../lib/ratchet.md) shows, and so is
the runner's side: the rows, the `missing` gate, the summary and `--repin`.
A producer is on the ratchet when `tests/ledger` holds a file for it. The
producers under § The producers are proposed.

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
| The instrument | `lib/ratchet.lisp` and its submodules | gauges, the estimator, the drives, the reading line, the judge, the re-pin's rewrite |
| The reading | one line on stdout | subject, axis, value, unit, uncertainty |
| The ledger | `tests/ledger/*.lisp`, committed | one row per (subject, axis): the bound, its kind, its class |
| The runner | `src/test/ledger.lisp` and `src/test/repin.lisp` | reading every line back, the completeness gate, history, `--repin` |

A producer imports the instrument, drives its shapes, and calls `report`. The
instrument judges each reading against the ledger as it lands, prints it, and
fails once at the end naming every reading that is not `ok`. Under `elle test`
the same lines become rows, the runner judges them again against the same
ledger, adds the rows no producer answered, and offers to move the ledger to
what was read.

### The reading line

Every reading is one line on stdout, opened by the word `measure` and carrying
one JSON object:

```text
measure {"subject":"io-drop","axis":"objects","value":0.0,"half":0.04,"unit":"objects/op","bound":0,"verdict":"ok"}
```

`subject` names the shape, `axis` the dimension, `value` the reading and
`unit` what one unit of it is. `half` is the half-width of the reading's
interval: what the estimator reports for a rate, and 0 for an exact count. The
instrument adds `bound` and `verdict` when it has judged the line; a producer
that only prints leaves them out and the runner judges.

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

A row belongs to a build. The reference build is the default build on Linux
x86_64, the one the `Default Build Tests` job runs, and a row with no `:build`
belongs to it. A pin is two-sided on the build it belongs to and one-sided on
every other build: there a reading past it the worse way is a regression and
a reading past it the better way is `ok`. So a build that reclaims more than
the reference build passes, exactly as it passed the ceilings the ratchet
replaced, and the pin stays tight where it was read. A build that reads worse
than the reference build gets a row of its own with `:build`, and that row is
two-sided there. On its build, a `:build` row replaces the row with none; on
every other build it is no row at all.

A build's key names its tier, its I/O backend, its operating system and its
architecture, as `(elle/build)` reports them: `jit-uring-linux-x86_64` is the
reference build, and `mlir-uring-linux-x86_64` and `jit-pool-macos-aarch64`
are two others. The rig's profiles and a file's sidecar change how a build
runs a file and never which build it is, so the eager pass of the
implementation suite judges against the same rows as the plain pass.

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

### The judge

Every reading meets its row, and the outcome is one of six verdicts:

| Verdict | Meaning | Gates |
|---------|---------|-------|
| `ok` | the reading is within its bound | no |
| `regression` | the reading moved the worse way | yes |
| `stale` | the reading moved the better way past the slack, on the row's own build; re-pin it | yes |
| `unledgered` | a reading with no row; adopt it or delete the measurement | yes |
| `missing` | a row whose producer ran and reported nothing | yes |
| `void` | the instrument's own check failed, so the reading says nothing | yes |

The comparison is on intervals. Take a reading `v ± h` against a pin `p` with
slack `s`, under `:better :lower`. It is a regression when `v - h > p + s`,
and stale when `v + h < p - s`. The sides swap under `:better :higher`. A
floor fails when `v + h` is below it and a ceiling when `v - h` is above it. So a rate
measured to a wide epsilon passes a pin it straddles, and only a rate measured
tightly enough to clear the pin can fail it. What the instrument can see is
what the gate can hold. The stale side is judged on the row's own build only:
away from it a pin is a ceiling under `:better :lower` and a floor under
`:better :higher`, and a reading past it the better way is `ok`.

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

`elle test` reads every `measure` line out of every captured stdout, judges
it, and writes one `measurement` row per reading:

```sql
CREATE TABLE measurement (
  run_id INT REFERENCES run(id),
  result_id INT REFERENCES result(id),   -- the form × tier, or the child, that printed it
  subject TEXT, axis TEXT,
  value REAL, half REAL, unit TEXT,
  bound REAL, kind TEXT,                 -- the row it met: pin|floor|ceiling, or NULL when unledgered
  verdict TEXT);                         -- ok|regression|stale|unledgered|missing|void
```

After each result lands as a `pass` it walks that file's ledger. A row the
result printed no reading for becomes a `missing` row against it. A producer
that gated itself out has skipped, not failed to report, and its rows are
left alone. A run with a selection can only hold the producers it ran, so the
full gate is a `selection IS NULL` run, as it is for every other verdict.

A producer with no ledger file is recorded and not judged: its rows carry no
verdict, and the gate ignores them. That is how a dashboard keeps its history
in the table before its ledger exists, and the first row written for it is
what starts the gate.

The summary counts readings by verdict and lists every one that is not `ok`:

```text
412 readings · 409 ok · 1 regression · 1 stale · 1 missing
  regression  tests/impl/oracle.lisp  [process]  reduce  objects  1.31 ±0.12 objects/op  pinned 1.002
  stale  tests/impl/plumb.lisp  [process]  ev-abort  regions  0.0 ±0.03 regions/op  pinned 1
  missing  tests/impl/oracle.lisp  [process]  fiber-nested  regions
```

The listing reads in the tally's order and names the tier, because a form
runs once per tier and each run's reading is a row of its own. The gate fails
on any verdict but `ok`, exactly as it fails on a form.

`elle test --repin PATHS` runs the selection, then moves the ledger to what it
read: every `stale` row takes the new reading, and every `unledgered` reading
becomes a row. It refuses a `regression`. A bound moves the worse way by a hand
edit, which the pull request's diff then shows beside the change that needed
it. The tool prints each row it moved. A reading it re-pins is written to three
significant figures for a rate and as the integer it is for a count.

A subject read on several tiers moves to the worst of its readings, so the
new pin holds on every tier. A `stale` row is one the running build owns, so
the tool moves the running build's rows and no other build's. An `unledgered`
reading of a growth class, which is the instrument's own live-growth row, is
adopted as a growth floor at the floor the instrument named; every other
`unledgered` reading is adopted as a pin at its value. A reading adopted on a
build other than the reference build is adopted as a `:build` row, so a
foreign reading never pins the reference build. The rewrite is the
instrument's, in [repin.lisp](../lib/ratchet/repin.lisp): it scans the
ledger's text for the row's brackets, replaces the bound's token, and appends
an adopted row after the last one.

A `run` row records the build's key, so a reading's history groups by the
build that read it ([test-store](test-store.md)).

The `ELLE_TEST_MEASUREMENTS` channel, `measurement-sink`, `measurement-env`
and the `closed`/`open`/`growth` verdict vocabulary are deleted. The runner
imports the instrument for the judge, the row reader and the re-pin, so the
runner and a direct run judge with one function.

The runner is also a producer. It reads its own three gauges at every file
boundary today and pins none of them. At the end of a run it reports, for the
producer `elle test`, the largest delta any one file charged on each gauge.
The ledger pins the three. A compiler change that raises every file's compile
cost moves the maximum, which is the regression this exists to catch, and the
per-file rows in `gauge` keep saying which file.

### The instrument

`(import "std/ratchet")` answers a closure; calling it makes one instrument
with its own readings and its own copy of the ledger, resolved for the running
program. The producer is the path the program was started with, the first
element of `(sys/argv)`, which is the same path the runner records for the
form. The ledger directory is `tests/ledger` under `(elle/root)`, the module
resolution root `std/` already resolves against, and `ELLE_LEDGER` names
another one. The build is what `(elle/build)` reports, and a test that wants
the instrument to judge as another build hands it one:
`((import "std/ratchet") :build "mlir-uring-linux-x86_64")`.

A form the runner runs in a worker thread was started with no path, so it has
no producer. The instrument then prints every reading without a bound or a
verdict, and the runner judges them. A direct run and an isolated child both
know their path, so both judge as they go.

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
| `(report)` | fail once, naming every reading that is not `ok` and every row of this producer that got no reading |

Every reading judges itself as it lands and prints its line, so a direct run
of a producer is the whole gate for that producer, with no runner present. The
runner adds what one process cannot: rows across producers, history across
commits, and the re-pin.

**The instrument proves each gauge live.** The first time a producer reads an
axis, the instrument drives that gauge's own live-growth shape first and
reports it as `<axis> gauge (live-growth)`. Its row is a growth floor, and a
floor that fails voids the axis. No producer writes a discriminator, and none
can forget one. A module-level sink keeps every struct it is handed, so the
object count, the region count and the byte count climb. A sink of pairs
keeps the free list drained, so the id counter climbs.

`report` raises one `:failed-assertion` with every failing line in its
message, so under the runner the producer's form fails with the same text the
`measurement` rows carry. A producer with nothing to say beyond its readings
ends with `report` and no other assertion.

## The producers

The producers proposed here are the ones the design has to specify because
nothing else does: each reads a number no test reads today. A test that
already measures joins the ratchet by importing the instrument and committing
its ledger, and needs no paragraph here; what it reads is that test's header,
and why its rows are what they are is its ledger's comments.

**The runner**, as above.

**The audit queue.** A producer, `tests/ratchet/audit.lisp`, runs
`scripts/audit` and reads two counts: the files with no stamp, and the files
stamped before the policy. Both are pins with `:better :lower`. The ratchet
the policy describes in prose becomes a row that fails when the count rises.

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
   `read`, `report`, the gauges, the judge and the row reader, and
   `(elle/root)`; `tests/ledger/` with a ledger for [the guide](../lib/ratchet.md),
   which is the library's own fixture. The counter-factual: a reading past its
   pin fails a direct run, and a reading past it the better way fails as
   stale.
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
- A pin is two-sided on the build it belongs to and one-sided elsewhere. One
  alternative judges every build two-sided against a row of its own, so a
  better reading on any build fails until that build has a row, and a macOS
  row can only be written from an imported CI store. The other lets a stale
  reading pass everywhere, which is the loose pin the copies had.
- Readings travel on stdout. The alternative keeps the environment-variable
  file, which no worker thread and no foreign producer can use.
- The bound lives in the ledger, never in the producer. The alternative keeps
  numbers in source and adds a tool that edits source, which the rewrite engine
  can do; it leaves coverage a convention and the raise unenforced.
- The instrument judges in-process, so a direct run of one producer is that
  producer's whole gate. The alternative makes `elle test` the only judge,
  which takes the verdict away from the command a developer iterates with.
