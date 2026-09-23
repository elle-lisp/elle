# Process scheduler

<!-- audited: 2026-09-23 -->

How a process scheduler runs sub-fibers, forwards its I/O to the scheduler it runs in, and nests.

[processes.md](processes.md) owns the process model: mailboxes, links,
monitors, timers and the clock. This file covers what a process scheduler does
with the fibers a process spawns and with the I/O they request.

## Structured concurrency inside processes

`ev/spawn` and `ev/join` work inside processes. Sub-fibers are
tracked by the scheduler and participate in I/O completion.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let* [f1 (ev/spawn (fn [] (+ 10 20)))
         f2 (ev/spawn (fn [] (+ 30 40)))
         r1 (ev/join f1)
         r2 (ev/join f2)]
    (assert (= r1 30) "f1 = 30")
    (assert (= r2 70) "f2 = 70"))))
```

`ev/select` — and everything built on it: `ev/timeout`, `ev/race`,
`ev/scope`, `ev/as-completed` — works from the process fiber and from
sub-fibers alike. The scheduler serves a process fiber's wait and a
sub-fiber's through the same code, so both have the full wait vocabulary.
A wait op the scheduler does not know is a protocol error, raised in the
fiber that emitted it rather than swallowed.

One rule keeps those waits honest: the scheduler never resumes a parked
fiber except with the result it parked for. A join or select on a
hand-built `:new` fiber gives that fiber a first run; a `:paused` fiber
is already parked on I/O, a futex, or a wait the scheduler tracks, and
is woken only by its own completion. Resuming it out of turn would hand
its park a nil — a timer parked in `ev/sleep` would "complete"
instantly, and every `ev/timeout` in a process would report its
deadline at once. Pinned by [process-select.lisp](../tests/elle/process-select.lisp).

A process that exits drops every futex park it made, from the process fiber
or from one of its sub-fibers. An `ev/futex-wake` after that exit counts and
wakes only the waiters that are still alive.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let* [me (process:self)
         bx (box 0)
         doomed (process:spawn (fn [] (ev/futex-wait :k bx 0)))
         live (process:spawn (fn []
                (ev/futex-wait :k bx 0)
                (process:send me :woken)))]
    (process:recv-timeout 2)
    (process:exit doomed :kill)
    (assert (= (ev/futex-wake :k 1) 1) "the wake takes one waiter")
    (assert (= (process:recv) :woken) "and that waiter is the live one"))))
```

## Orphan sub-fibers and teardown

A sub-fiber outlives the code that spawned it only as long as some
process is alive to observe it. Once **every** process has terminated,
any sub-fiber still running — a fire-and-forget `ev/spawn` that parked on
a futex, a background server blocked in `accept`, an un-joined worker
waiting on I/O — is an *orphan*: no live process can ever wake it or read
its result. `process:start` tears these orphans down (aborting them so
their `defer`/`protect` cleanup runs and cancelling their in-flight I/O)
rather than blocking forever waiting on work that can never complete.
This mirrors `ev/run`'s program-completion teardown for the root
scheduler (see [concurrency.md](concurrency.md)).

```text
(process:start (fn []
  ## fire-and-forget background server; the body never joins or stops it
  (ev/spawn (fn [] (protect (http2:serve listener handler))))
  nil))   ## body returns → server sub-fiber is orphaned and torn down
```

A sub-fiber the body **joins** (`ev/join`) is not an orphan: the process
stays alive until the join returns, so the sub-fiber completes first.

## Forwarded I/O and the parent scheduler

A process scheduler owns no I/O backend. It runs inside one fiber of its
parent scheduler, the scheduler that runs the code which called
`process:start` or `process:run`. The parent need not be the root. An I/O
request from a process — or from one of its sub-fibers — is *forwarded*: the
process scheduler hands the request up, the parent submits it, and the
completion comes back down.

The parent can only deliver a completion while the process scheduler is
suspended. So the process scheduler yields to its parent whenever every ready
process is merely refueling after fuel preemption. Without that yield, a
process that computes without pause holds the parent off and no completion
ever arrives.

The yield is bounded, and a ready process always gets to run again. The
scheduler never blocks until a forwarded completion arrives while a
process can still make progress, because the completion can *depend* on
that progress: an h2 client sub-fiber parked in `read` is waiting for the
request its own process has not finished sending. A process that never
gets to finish sending it would wait forever.

```text
(process:start (fn []
  ## The sleeper's completion is 30 s away; the loop below must not wait
  ## for it. Both finish, and the process ends as soon as the loop does.
  (let [sleeper (ev/spawn (fn [] (ev/sleep 30)))]
    (each i in (range 0 20000) (compute i))
    (ev/abort sleeper))))
```

When no process is ready, the scheduler blocks until a completion arrives.
While a timer is pending, it also forwards a sleep that ends when the earliest
timer falls due, so the wait ends no later than that. The clock counts the
wait, as [processes.md](processes.md) describes.

## Nested schedulers

Schedulers nest. A process can call `process:start` itself, and the outer
process scheduler is then the parent of the one it starts. The outer scheduler
relays each I/O request up to its own parent and each completion back down.
A request therefore crosses every scheduler between the process that made it
and the one that submits it. Each process scheduler keeps its own clock, so a
timer in the inner scheduler counts the inner scheduler's ticks.

When a process that runs a nested scheduler exits, its relayed I/O is cancelled
like any other I/O it had in flight.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:spawn (fn []
      (process:start (fn []
        (ev/sleep 0.001)
        (process:recv-timeout 5)))
      (process:send me :inner-done)))
    (assert (= (process:recv) :inner-done)
            "a scheduler inside a process does its I/O and finishes"))))
```

## See also

- [processes.md](processes.md) — the process model and its API
- [overview.md](../lib/process/overview.md) — how the scheduler's parts divide
- [concurrency.md](concurrency.md) — `ev/spawn`, `ev/join`, `ev/select`
- [scheduler.md](scheduler.md) — the async scheduler at the root
