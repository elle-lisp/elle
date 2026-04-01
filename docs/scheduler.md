# Scheduler

The async scheduler is the only supported execution backend. User code
runs inside it automatically — no setup required.

## Architecture

On Linux, the scheduler is a single-threaded event loop backed by
`io_uring`. On other platforms (macOS, CI), a threadpool-based
`SyncBackend` provides the same interface using blocking I/O on
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

---

## See also

- [concurrency.md](concurrency.md) — user-facing async primitives
- [fibers](signals/fibers.md) — fiber architecture
- [runtime.md](runtime.md) — runtime signals
- [io.md](io.md) — port I/O
