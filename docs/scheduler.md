# Scheduler

The async scheduler is the only supported execution backend. User code
runs inside it automatically — no setup required.

## Architecture

On Linux, the scheduler is a single-threaded event loop backed by
`io_uring`. On other platforms (macOS, CI), a threadpool-based
backend provides the same interface using blocking I/O on
background threads.

All I/O operations (port reads/writes, TCP, subprocess) yield to the
scheduler, which submits them to the backend and resumes the fiber
when the operation completes.

```text
┌─────────────┐
│  User fiber  │ ← ev/spawn creates these
│  (yield :io) │
└──────┬───────┘
       │ submit to io_uring
       ▼
┌─────────────┐
│  Event loop  │ ← io/wait polls completions
│  (io_uring)  │
└──────┬───────┘
       │ completion → resume fiber
       ▼
┌─────────────┐
│  User fiber  │ ← continues after yield
│  (result)    │
└─────────────┘
```

## ev/run

`ev/run` is the scheduler's entry point. The runtime calls it
automatically for user code. You rarely need to call it directly.

## io/wait

The scheduler's poll loop. Waits for `io_uring` completions and
resumes waiting fibers. Called internally by the event loop.

## Signal integration

I/O operations signal `:io` when they yield. The fiber's signal mask
must include `:io` (the async scheduler sets this up automatically
for spawned fibers).

## Timer support

`ev/sleep` and `ev/timeout` use `io_uring` timeout operations for
precise timer support without polling.

## Diagnosing threadpool hangs (`--trace=io`)

On the threadpool backend (macOS, or Linux with `--no-uring`), a blocking
`read`/`accept`/`write` runs on a worker thread that can only be woken by
data, EOF, or a `shutdown(fd)` from the close path — there is no async
cancel. A wakeup gap (notably: macOS `shutdown()` does **not** wake a
blocked `accept()` on a listening socket) can wedge the scheduler at
teardown until the CI `timeout` sends `SIGTERM`.

Run with `--trace=io` (or set `ELLE_TRACE=io` when the command line is
fixed, e.g. a CI job) to emit the threadpool op lifecycle
(`tp-submit`/`tp-complete`) plus close/cancel reaping to stderr via
async-signal-safe `write(2)`, so the last line before `SIGTERM` survives
and names the wedged op and fd. See the *Diagnostics* section of
[`src/io/AGENTS.md`](../src/io/AGENTS.md) for how to read the output.

---

## See also

- [concurrency.md](concurrency.md) — user-facing async primitives
- [processes.md](processes.md) — Erlang-style processes built on this scheduler
- [fibers](signals/fibers.md) — fiber architecture
- [runtime.md](runtime.md) — runtime signals
- [io.md](io.md) — port I/O
