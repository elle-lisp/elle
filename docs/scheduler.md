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

## Park queues

`ev/futex-wait` parks a fiber on a key. `ev/futex-wake` wakes up to
`count` of the fibers parked on that key. The scheduler keeps one queue
per key.

Two invariants govern the queues:

- **Only live fibers wait.** A fiber that reaches `:dead` or `:error`
  leaves every queue it sits in. A terminated fiber left in a queue takes
  a wake slot from a live waiter, so `(ev/futex-wake key 1)` reports a
  wake that no fiber received. The single-permit wake is the common case:
  `lib/http2/stream.lisp` wakes one taker per channel put, and
  `lib/http2/session.lisp` wakes one waiter per SETTINGS ACK.
- **An empty queue has no key.** The scheduler drops a key once its queue
  empties. The event loop reports `:done` only when no fiber waits on
  I/O, a join, a select, or a park, so a key that outlives its last
  waiter keeps the loop running with nothing left to run.

`ev/abort` and `ev/timeout` both terminate fibers that may be parked, so
both rely on these invariants. `tests/elle/park-abort.lisp` pins them.

---

## ev/report

`ev/report` returns what the running scheduler is waiting for, as a
struct:

| Field | Meaning |
|---|---|
| `:runnable` | fibers queued to run on the next drain |
| `:io` | submitted I/O operations with no completion yet |
| `:joins` | fibers with at least one join waiter |
| `:selects` | fibers parked on a select set |
| `:forwarded` | I/O submitted for a child scheduler |
| `:parks` | one `[key count]` pair per non-empty park queue |

The loop blocks when `:runnable` is empty and everything else is not, so
a report taken from a fiber the scheduler still runs names the waits that
outlived the work. A timer is the reliable way to take one: a sleep
completion arrives on its own and needs no other fiber to make progress,
so a watchdog spawned as `(ev/spawn (fn [] (ev/sleep n) (ev/report)))`
reports even when every other fiber is parked.

The park keys are whatever the caller of `ev/futex-wait` passed —
`lib/http2` uses a `(gensym)` per channel and per flow-control window, so
a count above one on a single key means several fibers wait on one
channel.

`tests/elle/sched-report.lisp` pins the shape.

## See also

- [concurrency.md](concurrency.md) — user-facing async primitives
- [processes.md](processes.md) — Erlang-style processes built on this scheduler
- [fibers](signals/fibers.md) — fiber architecture
- [runtime.md](runtime.md) — runtime signals
- [io.md](io.md) — port I/O
