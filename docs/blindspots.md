# Blind spots

<!-- audited: 2026-11-23 -->

What `elle test` and the runtime do not measure about memory, leaks, compilation and the JIT.

A blind spot is a quantity that no run records and no gate judges. Each entry
names one, says where the nearest instrument stops, and links the document that
owns that instrument. The change that closes a blind spot deletes its entry
here, and documents the new instrument where its subject lives.

## Memory

| Blind spot | Where the instruments stop |
|------------|----------------------------|
| The resident memory of an in-process form | A worker thread shares the runner's process, so `result.max_rss_kb` is NULL on every in-process row. Only an `--isolate` child has a peak ([test-store](test-store.md)). |
| The memory of a run that dies | A run reads its process's peak at completion ([test-store](test-store.md)). A run the OOM killer ends records no peak, so the one run whose memory matters most reads NULL. |
| Memory outside the region store | The heap gauges read the region store alone ([test-gauges](test-gauges.md)). JIT code, the Cranelift modules that hold it, and the WASM handle table move no gauge ([diagnostics](impl/region/diagnostics.md)). |
| A worker past its deadline | The runner abandons the thread, which keeps its heap and can keep running. Its readings are lost, and nothing counts what it holds for the rest of the run. |
| A bound on what a run or a child costs | A finished run and an `--isolate` child each record their CPU time and their peak resident set. No ledger row judges any of them, so a run that doubles its peak still passes. |

## Leaks

| Blind spot | Where the instruments stop |
|------------|----------------------------|
| A leak in an ordinary test | Nothing bounds a `gauge` row, so the exit code ignores what a file left on the runner heap or the test heap. [test-gauges](test-gauges.md) says why a delta cannot be judged. |
| Region and page growth per file | `--charge` judges the objects a file leaves live, and nothing else. [test-gauges](test-gauges.md) says why regions and pages wait. |
| Region leaks outside the dashboards | The ratchet judges a leak rate only where a producer samples its own gauges and prints a reading ([ratchet](ratchet.md)). A test that prints no reading leaks unjudged. |
| The runner's teardown | The runner ends in `os/exit`, which calls `std::process::exit`, so its teardown sweep never runs. The residue that `--dump=stats` counts and `--trace=residue` names goes unread ([diagnostics](impl/region/diagnostics.md)). |
| A child's teardown | A child that `--isolate` starts with `--dump=stats` prints its residue count to stderr. The runner keeps stderr as an asset, and reads readings from stdout alone. |
| An isolated child's heap | The child writes no `gauge` row ([test-gauges](test-gauges.md)). |

## Compilation

| Blind spot | Where the instruments stop |
|------------|----------------------------|
| How many units an instance compiled | No counter exists. No `vm/query` operation and no `--dump=stats` line counts the front end's compiles, expansions or lowerings. |
| The CPU time of a compile | No clock inside the runtime reads it. The runner reads one around each compile on its own thread ([test-store](test-store.md)). |
| The wall time of a compile | `--trace=compile` prints a wall-clock mark per phase to stderr ([config](config.md)), and nothing records the marks. The test modes mark the front end, and their lowering and emission print no mark. |
| What a worker's compile costs a run | The runner times the compiles on its own thread, and not the one in a worker (below). |

The runner compiles a file more than once. A single-form file compiles on the
runner's thread before the clock of its first result starts. A multi-form file
compiles once on the runner's thread, to find a compile error, then once in
each worker that runs it.

The worker's compile falls inside the result's `wall_ms` and outside its
`cpu_us` ([test-store](test-store.md)). It also falls outside the test-heap
readings, which bracket the run of the file alone, and no clock times it on
its own.

## The JIT

| Blind spot | Where the instruments stop |
|------------|----------------------------|
| A JIT compile in flight when its form ends | A VM counts a compile when it takes the result ([jit](impl/jit.md)). The runner reads the count as the form ends, so a compile still on the `elle-jit` thread counts in no result. |
