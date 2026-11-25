# What a test run costs

<!-- audited: 2026-11-24 -->

The time and memory `elle test` records for a run, a result and a compile, and what each reading covers.

Where the rows live is [test-store](test-store.md), and what each file cost a
heap is [test-gauges](test-gauges.md). What no reading covers yet is
[blindspots](blindspots.md).

## A run

A run records its wall time from insert (`wall_ms`), and what the runner's
process cost, read from `sys/rusage`: its peak resident set (`max_rss_kb`), and
its CPU time in user mode and in the kernel (`cpu_user_ms`, `cpu_sys_ms`). That
process runs every worker, the JIT's thread and the I/O pool, so the three
cover them all. They cover no `--isolate` child. They count from the start of
the process, its boot included, where `wall_ms` counts from the run's insert.
The runner writes all four after each file's rows and gauges, and again in the
completion stamp. A run that dies keeps the reading its last finished file
took, so its peak is a lower bound. A run that dies in its first file records
none of them.

## A result

A result that a form or a child produced records what it cost. For a form in
a worker, `wall_ms` runs from handing the form over to having its answer, and
`cpu_us` is the delta of `(clock/cpu)` on the thread that ran it. That
thread's CPU leaves out the JIT's compile thread, the I/O pool and any child
the form starts. For an `--isolate` child, `wall_ms` runs from spawn to reap,
and `cpu_us` (user plus system) and `max_rss_kb` are its total from
`subprocess/rusage` ([subprocess](subprocess.md)).

Under `elle-rig test`, a form also records the JIT compiles its VM took over
the window `cpu_us` covers, read with the rig's `jit/stats`
([rig](../rig/overview.md)): `jit_compiles`, how many of them produced no code
in `jit_rejected`, and their wall and CPU time in `jit_wall_us` and
`jit_cpu_us`. A compile on the form's own thread, as `compile/run-on :jit` runs
one, counts in `cpu_us` as well. A form the runner runs in-process reads the
runner's VM, so the runner's own compiles in that window count with it. A run
under `elle test` and an `--isolate` child leave the four NULL.

An `--isolate` child also records `residue_regions`: the regions its teardown
left alive, which the target holds at zero ([diagnostics](impl/region/diagnostics.md)).
The runner starts every child with `--dump=stats` ([test-runner](test-runner.md)),
and reads the count from the `[stats] live regions after teardown: N` line the
child prints to stderr. The teardown runs after the program, so the last such
line is the child's own, whatever its program printed first. A child that
prints none leaves the column NULL: one that was killed, that ended in
`os/exit`, or whose program prints no such line. A form leaves it NULL too.

A multi-form file's result also records the compile its run needed. The
worker compiles the file's syntax against its own stdlib before it runs it
([test-runner](test-runner.md)), and times that compile into
`compile_wall_us` by the clock and `compile_cpu_us` by `(clock/cpu)` on its
thread. The compile falls inside the result's `wall_ms`, and outside its
`cpu_us` and its JIT columns. The worker also reads the heap gauges around the
compile alone ([test-gauges](test-gauges.md)). A script that falls back to the
runner's process compiles there, and records that compile the same way. A
single-form result leaves both columns NULL, because its worker runs a thunk
the runner compiled. An `--isolate` child leaves them NULL too, because the
child times nothing it hands back.

A worker that never hands back its answer — a missed deadline, a panic —
leaves `cpu_us`, the JIT columns and the compile columns NULL. A file-level
error or skip and a divergence row leave every cost column NULL, because
nothing ran to produce them.

CPU time is a `(clock/cpu)` delta, because fuel counts continuation-passing
rather than work. It is not deterministic, so a query compares it across many
runs rather than for equality.

## The residue, for the ratchet

Under `elle-rig test`, a child that passed gives one reading of its residue
([ratchet](ratchet.md)). The producer is `elle test`, the subject is the path,
the axis is `residue`, and the unit is `regions`. A child that failed, timed
out or skipped gives no reading and owes no row: a failure already gates, and a
skip ran nothing the row describes. A child that passed and printed no residue
line gives no reading, so its row is `missing`.

A run owes a file the row of its own pass alone. An isolated run owes the
file's `residue` row, and a `--charge` run its `objects` row
([test-gauges](test-gauges.md)). A build with no rows of its own records the
reading and judges nothing, so only the reference build gates on it.

The residue is the reading because it repeats. Every file of the
implementation suite, the runner's tests and the command line's tests read
the same in two runs, under the eager profile, and on CI's machine. Of the
language suite, one file moves: [tls.lisp](../tests/lang/tls.lisp), by one
region. `tls/read-all` ([lib/tls.lisp](../lib/tls.lisp)) leaves each chunk it
reads alive, and how TCP splits the stream decides how many chunks it reads.
That row carries a slack that covers what it reads ([ratchet](ratchet.md)). The
CPU time and the peak resident set do not repeat: between two identical runs
of the suites, one file's peak moved by a factor from 0.43 to 2.00, and the CPU
time of one file in seven by more than half. Neither has a row
([blindspots](blindspots.md)).

`make smoke-impl` reads the implementation suite, the runner's tests and the
command line's tests, and `make smoke-eager` reads the language suite and the
implementation suite under the eager profile. A profile never changes the
build, so the two passes judge an implementation file against one row. A new
file reads `unledgered` until its row is adopted, under the pass that reads
it:

```sh
target/release/elle-rig test --isolate '' --repin tests/impl/NEW.lisp
target/release/elle-rig test --repin \
  --isolate '--profile tests/impl/profiles/jit-eager.toml' tests/lang/NEW.lisp
```

## The runner's compiles

The runner compiles each file on its own thread before a worker runs it, and
times each compile into a `compile_cost` row: `wall_us` by the clock, and
`cpu_us` by `(clock/cpu)` on that thread. The cost belongs to a file, as a
gauge does ([test-gauges](test-gauges.md)).

| Step | The compile |
|------|-------------|
| `profile` | `compile/analyze`, for the analysis columns ([test-store](test-store.md)) |
| `module` | `compile/barrier-module` or `compile/whole-module`, to find a compile error |

A compile that fails keeps its row, and a path the runner cannot read has none.
Under `--isolate` the child compiles the module, so the runner writes a
`profile` row alone. A multi-form file compiles once more in each worker that
runs it, and that compile belongs to the worker's result (§ A result).
