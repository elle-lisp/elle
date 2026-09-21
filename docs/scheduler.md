# Scheduler

<!-- audited: 2026-09-20 -->

The async scheduler is the only supported execution backend, and user code runs inside it automatically.

No setup is required.

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
  [stream.lisp](../lib/http2/stream.lisp) wakes one taker per channel put, and
  [session.lisp](../lib/http2/session.lisp) wakes one waiter per SETTINGS ACK.
- **An empty queue has no key.** The scheduler drops a key once its queue
  empties. The event loop reports `:done` only when no fiber waits on
  I/O, a join, a select, or a park, so a key that outlives its last
  waiter keeps the loop running with nothing left to run.

`ev/abort` and `ev/timeout` both terminate fibers that may be parked, so
both rely on these invariants. [park-abort.lisp](../tests/elle/park-abort.lisp) pins them.

## Join waiters and select sets

`ev/join` parks a fiber on the waiter list of the fiber it joined.
`ev/select` parks it in a select set naming several candidates. A fiber
that completes resumes every waiter on its own list, and one waiter from
every select set that names it.

Three invariants govern both lists:

- **Only live fibers wait.** A fiber that reaches `:dead` or `:error`
  leaves the waiter list and the select set it sits in, on the rule that
  takes it out of a park queue. A joiner reaches `:dead` with its wait
  still recorded whenever `fiber/abort` injects an error its own
  `protect` catches. Left on the list, it is resumed once the fiber it
  joined finishes, and that resume raises `fiber/resume: cannot resume
  completed fiber` out of the event loop.
- **A list with no waiter left is gone.** An empty waiter list still
  counts as a join the loop is holding, so the loop never reports
  `:done`, exactly as an empty park key would keep it running.
- **A delivery reads the list, never a copy of it.** Resuming one waiter
  runs that waiter's own code before the next waiter is reached, and
  that code can kill a sibling — `ev/abort` on a fiber waiting for the
  same result is enough. The sibling leaves the list on the rule above,
  which a copy taken before the first resume would not show.

The scheduler holds the waiter list in two halves, the pairing it already
keeps for a park: the joined fiber maps to its waiters, and each waiting
fiber maps to the one fiber it joined. The second half is what lets a
dying joiner leave its list without searching the others. A select set is
keyed by the waiting fiber itself, so it needs no second half.

`(ev/timeout n (fn [] (protect (ev/join-protected f))))` is the shape a
program reaches this through — a deadline around a protected join, where
`f` outlives the deadline. `ev/timeout` selects over the body and a
timer, so one such call puts fibers in both lists.
[abort-wait-lists.lisp](../tests/elle/abort-wait-lists.lisp) pins them,
and reads the counts back through `ev/report`'s `:joins` and `:selects`.

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
  held — an operation whose operands are gone has no reader either
  ([the io backend rules](../src/io/AGENTS.md)). The scheduler drops that
  error exactly as it would a result. An abort retains those regions instead,
  because the unwinding it starts can suspend and be resumed
  ([the fiber primitives](signals/primitives.md)).
- **A finished fiber holds no operation.** Completing a fiber cancels the
  submission it still waits on. Otherwise that submission keeps a worker
  and a descriptor for a fiber that can never read the result, and the
  loop keeps waiting on a completion nobody wants.

[io-late-completion.lisp](../tests/elle/io-late-completion.lisp) pins both over a portless timer, and
[io-stale-operation-ends.lisp](../tests/elle/io-stale-operation-ends.lisp) over a port operation whose
operands are gone — the case where the entry holds values to read.

## Completion records

The scheduler remembers the fibers it has finished with: a status record
(`:ok` or `:error`) per completed fiber, and a mark per fiber whose
result someone took. Both are keyed by the fiber, so a record holds the
fiber value — and the region the fiber and its closure live in — for as
long as the record lasts.

Four invariants govern the records:

- **A record lasts only as long as a reader needs it.** Both records go
  the moment nothing can read them again. For a fiber that ends in
  success that moment is completion, joined or not: nothing reads a
  success record at all. For a fiber that **failed** it is the moment
  somebody observes the failure — a join, a protected join, or an abort,
  each of which marks the fiber. A failure record has exactly one reader,
  the unjoined-error tail at the end of the loop, and the tail passes
  over every failure that carries a mark. A record that outlives its
  readers makes every `ev/spawn` a permanent allocation, which a
  long-running program pays for once per fiber it ever ran — and
  `ev/timeout` and `ev/race` abort the loser on every call.

- **An absent record is re-derived from the fiber, the way the loop
  routes one.** An error does not unwind a fiber: it suspends holding the
  error signal, so `fiber/status` answers `:paused` — what a fiber
  waiting to resume answers — and only the `SIG_ERROR` bit tells the two
  apart. The re-derivation reads that bit, which is the reading
  `fiber-failed?` states and the one the loop already routes a completed
  fiber by. Reading the status alone answers "still running" for every
  retired failure, which parks a later joiner on a fiber nothing will
  resume.

- **A fiber the loop has finished with is not runnable.** Completing a
  fiber takes it out of the runnable queue, on the rule that takes it out
  of a park queue and a waiter list. Left there it is resumed once more
  and completes a second time. That second completion writes a fresh
  record for a failure whose mark the first retirement dropped, and the
  tail reads the fresh record as a failure nobody observed.

- **The loop's last act is to forget every fiber.** When `:pump` returns,
  every fiber the loop knows about is terminal and nothing will resume one,
  so no record can be read again — including the program's own, whose
  reader is the loop itself. Every record goes: the status records and
  marks, the join and select waiter lists, the submission pairing, the park
  queues, and the runnable queue.

  Held past that point they are not merely dead weight. Each record holds
  its fiber, a fiber holds the scheduler struct through the parameter
  baseline it was created with, and the struct's closures capture the very
  tables the records live in. That is a reference cycle through mutable
  edges, and per-region RC cannot break one — so the whole loop, every
  closure in it, and everything any of them reaches survives to process
  teardown, on every run of every program (elle-lisp/elle#1081).

  A join that arrives afterward loses nothing: an absent record is
  re-derived from the fiber, which is the same route every fiber retired
  at completion already takes.

The program's own fibers — the thunks `ev/run` hands the loop — are the
one exception to the first invariant. Their records are what tells the
loop the program finished, so they last until the loop ends.

[sched-completion-records.lisp](../tests/elle/sched-completion-records.lisp) pins the bound through
`ev/report`'s `:records` / `:marks`, and that the pump leaves none of them
behind; [ev-unjoined-error.lisp](../tests/elle/ev-unjoined-error.lisp) pins that retiring the records
still leaves an unjoined failure to crash the program.
[plumb.lisp](../tests/elle/plumb.lisp) reads what an abort costs as a rate, and
[region_process_teardown.rs](../tests/region_process_teardown.rs) pins what all of it is worth: a
completed run leaves no live region at all.

---

## ev/report

`ev/report` returns what the running scheduler is waiting for, as a
struct:

| Field | Meaning |
|---|---|
| `:runnable` | fibers queued to run on the next drain |
| `:io` | submitted I/O operations with no completion yet |
| `:workers` | those operations that a background worker is busy with, idle workers excluded (zero on io_uring, which runs them in the kernel) |
| `:joins` | fibers with at least one join waiter |
| `:selects` | fibers parked on a select set |
| `:forwarded` | I/O submitted for a child scheduler |
| `:records` | completed fibers whose status the loop still holds |
| `:marks` | fibers marked as observed (joined, aborted, or the program's own) |
| `:parks` | one `[key count]` pair per non-empty park queue |

`:records` and `:marks` are the completion bookkeeping above, not waits: a
program that spawns in a loop reads them to see that finished fibers are let
go. They stay flat under a join-and-discard loop, under an abort loop, and
under a spawn loop that joins nothing; they grow only with failures nobody
has observed yet.

The loop blocks when `:runnable` is empty and everything else is not, so
a report taken from a fiber the scheduler still runs names the waits that
outlived the work. A timer is the reliable way to take one: a sleep
completion arrives on its own and needs no other fiber to make progress,
so a watchdog spawned as `(ev/spawn (fn [] (ev/sleep n) (ev/report)))`
reports even when every other fiber is parked.

The park keys are whatever the caller of `ev/futex-wait` passed —
[http2](../lib/http2/AGENTS.md) uses a `(sys/unique)` per channel and per flow-control window, so
a count above one on a single key means several fibers wait on one
channel.

[sched-report.lisp](../tests/elle/sched-report.lisp) pins the shape.

## See also

- [concurrency.md](concurrency.md) — user-facing async primitives
- [processes.md](processes.md) — Erlang-style processes built on this scheduler
- [fibers](signals/fibers.md) — fiber architecture
- [runtime.md](runtime.md) — runtime signals
- [io.md](io.md) — port I/O
