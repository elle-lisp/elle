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

## Completion delivery

A completion names the operation, not the fiber. The scheduler holds the
pairing in two halves: a submission id maps to the fiber that asked for
it, and the fiber maps to the one submission it waits on. Both halves go
away together — when the completion arrives, when `ev/abort` cancels the
operation, and when the scheduler finishes with the fiber.

Two invariants govern delivery:

- **A completion reaches only a fiber still waiting for it.** A fiber can
  terminate by a path the scheduler did not route. `fiber/abort` injects
  an error the fiber's own `protect` may catch, so the fiber runs to
  `:dead` while its read is still in flight. When that operation
  completes, the scheduler records the fiber's completion and drops the
  result instead of resuming it. Resuming a fiber that already finished
  raises `fiber/resume: cannot resume completed fiber` out of the event
  loop, which reaches the program as a runtime error with no connection
  to the fiber that died.

  A `fiber/cancel` ends the same way and releases more. It gives the fiber
  no chance to recover, so the regions holding the operation's operands —
  the port, the buffer the read reserved — go with it. The backend then
  retires the entry unread and answers with an error built from nothing it
  held (`src/io/AGENTS.md` § "An operation whose operands are gone has no
  reader either"). The scheduler drops that error exactly as it would a
  result. An abort retains those regions instead, because the unwinding it
  starts can suspend and be resumed (`docs/signals/primitives.md`
  § "Unwinding that suspends").
- **A finished fiber holds no operation.** Completing a fiber cancels the
  submission it still waits on. Otherwise that submission keeps a worker
  and a descriptor for a fiber that can never read the result, and the
  loop keeps waiting on a completion nobody wants.

`tests/elle/io-late-completion.lisp` pins both over a portless timer, and
`tests/elle/io-stale-operation-ends.lisp` over a port operation whose
operands are gone — the case where the entry holds values to read.

## Completion records

The scheduler remembers the fibers it has finished with: a status record
(`:ok` or `:error`) per completed fiber, and a mark per fiber whose
result someone took. Both are keyed by the fiber, so a record holds the
fiber value — and the region the fiber and its closure live in — for as
long as the record lasts.

One invariant governs the records:

- **A record lasts only as long as a reader needs it.** A delivered join
  or abort drops both records for that fiber. Nothing reads them
  afterwards: the status is re-derived from the fiber itself whenever the
  record is absent, the value was always read from the fiber rather than
  from the record, and the unjoined-error tail at the end of the loop
  looks only at fibers nobody joined. A record that outlives its readers
  makes every `ev/spawn` a permanent allocation, which a long-running
  program pays for once per fiber it ever ran.

The program's own fibers — the thunks `ev/run` hands the loop — are the
exception. Their records are what tells the loop the program finished,
so they last until the loop ends.

`tests/elle/sched-completion-records.lisp` pins the bound through
`ev/report`'s `:records` / `:marks`; `tests/elle/ev-unjoined-error.lisp` pins
that retiring the records still leaves an unjoined failure to crash the
program.

The records are not the only per-fiber cost. A spawned fiber still strands a
few regions of its own after everyone has let go of it — the residue
`oracle.lisp`'s `spawn-join` probe measures and bounds. That is a region-model
defect, tracked there; these records are the scheduler's own half of it.

---

## ev/report

`ev/report` returns what the running scheduler is waiting for, as a
struct:

| Field | Meaning |
|---|---|
| `:runnable` | fibers queued to run on the next drain |
| `:io` | submitted I/O operations with no completion yet |
| `:workers` | background worker threads out for those operations (zero on io_uring, which runs them in the kernel) |
| `:joins` | fibers with at least one join waiter |
| `:selects` | fibers parked on a select set |
| `:forwarded` | I/O submitted for a child scheduler |
| `:records` | completed fibers whose status the loop still holds |
| `:marks` | fibers marked as observed (joined, aborted, or the program's own) |
| `:parks` | one `[key count]` pair per non-empty park queue |

`:records` and `:marks` are the completion bookkeeping above, not waits: a
program that spawns in a loop reads them to see that finished fibers are let
go. They stay flat under a join-and-discard loop and grow only with fibers
nobody has observed yet.

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
