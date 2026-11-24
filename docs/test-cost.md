# What a test run costs

<!-- audited: 2026-11-23 -->

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

A worker that never hands back its answer — a missed deadline, a panic —
leaves `cpu_us` and the JIT columns NULL. A file-level error or skip and a
divergence row leave every cost column NULL, because nothing ran to produce
them.

CPU time is a `(clock/cpu)` delta, because fuel counts continuation-passing
rather than work. It is not deterministic, so a query compares it across many
runs rather than for equality.

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
`profile` row alone. A worker's compile is timed nowhere
([blindspots](blindspots.md)).
