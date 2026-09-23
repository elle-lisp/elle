# Processes

<!-- audited: 2026-09-23 -->

[lib/process.lisp](../lib/process.lisp) provides Erlang-style concurrent processes built on
Elle's fiber scheduler. Processes have mailboxes, links, monitors, named
registration, and fuel-based preemption.

The callback-driven roles built on that model — GenServer, Actor, Task and
EventManager — are in [behaviors.md](behaviors.md), and the supervisor that
restarts them is in [supervisor.md](supervisor.md).

## Loading

```lisp
(def process ((import "std/process")))
```

## Starting a process system

`process:start` creates a scheduler and runs a closure as the first
process, PID 0. It blocks until no process can run again and returns the
scheduler.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (println "hello from process 0")))
```

A process that outlives PID 0 keeps the scheduler running while it has work.
When PID 0 has ended and every process left waits in a receive that no
message, timer or I/O can satisfy, those processes are idle for good.
`process:start` then exits each of them with reason `:shutdown` and returns.
While PID 0 itself waits that way, nothing can end the program, and
`process:start` raises `{:error :deadlock}`.

```lisp
(def process ((import "std/process")))

## PID 0 returns while the process it linked waits in recv.
(process:start (fn []
  (process:spawn-link (fn [] (process:recv)))
  :done))
```

PID 0 can end with an exit reason it did not choose: `[:error e]` when it
raises, `[:linked pid reason]` when a linked process kills it, or
`[:killed reason]` when another process calls `exit` on it. For each of
these, `process:start` raises `{:error :process-error :reason r}` once the
scheduler stops, where `r` is the exit reason as a string.

```lisp
(def process ((import "std/process")))

(let [[ok? err] (protect (process:start (fn []
                  (process:spawn-link (fn [] (error {:error :boom :message "crash"})))
                  (process:recv))))]
  (assert (not ok?) "a crash that kills PID 0 reaches the caller")
  (assert (= (get err :error) :process-error) "as a process error"))
```

`make-scheduler` and `start` take `:fuel`, the instructions a process runs
before it is preempted, 1000 by default. Neither builds an I/O backend, because
a process scheduler forwards its I/O to the root scheduler (see "Forwarded I/O
and the root scheduler" below).

Use `process:run` when you need a pre-configured or shared scheduler:

```text
(def sched (process:make-scheduler :fuel 500))
(process:run sched (fn [] (println "on existing scheduler")))
```

## Sending and receiving messages

Every process has a mailbox. `send` delivers a message; `recv` blocks
until one arrives.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:send me :hello)
    (assert (= (process:recv) :hello) "got :hello"))))
```

## Spawning processes

`spawn` creates a new process. `spawn-link` links the child to the
parent (crash propagation). `spawn-monitor` monitors without linking
(death notification without crashing the parent).

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let* [me (process:self)
         peer (process:spawn (fn []
                 (match (process:recv)
                   [from :ping] (process:send from :pong)
                   _ nil)))]
    (process:send peer [me :ping])
    (assert (= (process:recv) :pong) "ping-pong works"))))
```

## Selective receive

`recv-match` takes a predicate and returns the first message that
matches, leaving non-matching messages in the mailbox in order.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:send me :a)
    (process:send me :b)
    (process:send me :c)
    # Pick :b out of order
    (assert (= (process:recv-match (fn [m] (= m :b))) :b) "got :b")
    # Remaining arrive in original order
    (assert (= (process:recv) :a) "got :a")
    (assert (= (process:recv) :c) "got :c"))))
```

`recv-timeout` returns `:timeout` if no message arrives within the
given number of scheduler ticks.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (assert (= (process:recv-timeout 1) :timeout) "timed out")))
```

The scheduler keeps time in ticks. Each round of the scheduler, which runs
every ready process once, adds one tick. When every process waits on a timer,
the clock jumps to the earliest one. `now` returns the current tick, and
`recv-timeout`, `send-after` and a supervisor's `:max-ticks` all count it.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [before (process:now)]
    (process:recv-timeout 10)
    (assert (>= (- (process:now) before) 10) "recv-timeout waited 10 ticks"))))
```

## Links and crash propagation

A link ties two processes' exits together. When a process exits, every
process linked to it receives an exit signal that carries the exit reason:

- A process that traps exits receives the signal as the message
  `[:EXIT pid reason]`.
- A process that does not trap exits ignores a normal reason: `[:normal
  value]`, which a process that returns exits with, or `:normal`. Any other
  reason kills it, with the reason `[:linked pid reason]`.

A process that a link kills exits like any other. Its registered name is
released, its monitors receive `:DOWN`, and its own links receive the signal
in turn.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:trap-exit true)
  (let [child (process:spawn-link (fn []
                 (error {:error :boom :message "crash"})))]
    (match (process:recv)
      [:EXIT pid [:error _]] (assert (= pid child) "EXIT from child")
      _ (assert false "expected [:EXIT child [:error ...]]")))))
```

A normal exit leaves a linked process running when that process does not trap
exits:

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let* [me (process:self)
         peer (process:spawn (fn []
                (process:recv)
                (process:send me :peer-alive)))]
    (process:spawn (fn [] (process:link peer) :done))
    (process:recv-timeout 5)
    (process:send peer :go)
    (assert (= (process:recv) :peer-alive) "the peer outlived its partner"))))
```

`link` to a process that has already exited, and that the caller was not
linked to, sends the caller the exit signal `noproc`. A caller that traps
exits receives `[:EXIT pid :noproc]`. In any other caller, `link` raises
`{:error :noproc}`.

`exit pid reason` sends a process an exit signal directly. A target that traps
exits receives `[:EXIT sender reason]`; any other target dies with the reason
`[:killed reason]`. The reason `:kill` kills even a target that traps exits.

## Monitors

Monitors deliver a `[:DOWN ref pid reason]` message when the monitored
process dies, without affecting the monitoring process.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [[child-pid ref] (process:spawn-monitor (fn [] :done))]
    (match (process:recv)
      [:DOWN got-ref got-pid [:normal val]]
        (begin
          (assert (= got-ref ref) "correct ref")
          (assert (= got-pid child-pid) "correct pid")
          (assert (= val :done) "normal exit carries the return value"))
      _ (assert false "expected [:DOWN ref pid [:normal :done]]")))))
```

`monitor` on a process that has already exited delivers `[:DOWN ref pid
:noproc]` at once, so a monitor always ends in exactly one `:DOWN`.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [[pid _] (process:spawn-monitor (fn [] :done))]
    (process:recv)
    (let [ref (process:monitor pid)]
      (assert (= (process:recv) [:DOWN ref pid :noproc]) "late monitor gets :noproc")))))
```

`demonitor ref` stops a monitor. With `:flush true`, it also removes a
`[:DOWN ref …]` message that already arrived for that monitor.

## Named processes

Processes can register under a keyword name. `whereis` looks up PIDs
by name; `send-named` sends to a registered name.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:spawn (fn []
      (process:register :greeter)
      (let [msg (process:recv)]
        (match msg
          [from name] (process:send from (string "hello, " name))
          _ nil))))
    # sync to let the child register
    (process:send me :sync)
    (process:recv)
    (process:send-named :greeter [me "elle"])
    (assert (= (process:recv) "hello, elle") "named send works"))))
```

## Process dictionary

Each process has a private key-value store. Useful for per-process
configuration that doesn't belong in the main state.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:put-dict :counter 0)
  (process:put-dict :counter 42)
  (assert (= (process:get-dict :counter) 42) "dict works")
  (process:erase-dict :counter)
  (assert (nil? (process:get-dict :counter)) "erased")))
```

## Fuel-based preemption

Processes are cooperatively scheduled with fuel budgets. A CPU-bound
process gets preempted after exhausting its fuel, allowing other
processes to run.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    # Busy-looper gets preempted
    (let [busy (process:spawn (fn []
            (letrec [loop (fn [n] (loop (+ n 1)))] (loop 0))))]
      (process:spawn (fn [] (process:send me :done)))
      (assert (= (process:recv) :done) "worker runs despite busy-looper")
      (process:exit busy :kill))))
  :fuel 100)
```



# Structured concurrency inside processes

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

## Forwarded I/O and the root scheduler

A process scheduler owns no I/O backend. It runs inside one fiber of the
root scheduler, so an I/O request from a process — or from one of its
sub-fibers — is *forwarded*: the process scheduler hands the request up,
the root scheduler submits it, and the completion comes back down.

The root scheduler can only deliver a completion while the process
scheduler is suspended. So the process scheduler yields to the root
whenever every ready process is merely refueling after fuel preemption.
Without that yield, a process that computes without pause holds the root
off and no completion ever arrives.

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


# Process API reference

## Core

| Function | Description |
|----------|-------------|
| `start init` | Create scheduler, run init as first process |
| `run sched init` | Run init on existing scheduler |
| `make-scheduler` | Create scheduler (`:fuel`) |
| `self` | Current process PID |
| `spawn fn` | Start new process |
| `spawn-link fn` | Start linked (crash propagation) |
| `spawn-monitor fn` | Start monitored (death notification) |
| `send pid msg` | Send message |
| `recv` | Block until message arrives |
| `recv-match pred` | Receive first matching message |
| `recv-timeout ticks` | Receive with timeout |

## Links and monitors

| Function | Description |
|----------|-------------|
| `link pid` | Link to another process |
| `unlink pid` | Remove link |
| `monitor pid` | Monitor another process, returns a ref |
| `demonitor ref` | Remove monitor (`:flush`) |
| `trap-exit flag` | Catch linked exits as messages |
| `exit pid reason` | Send an exit signal to a process |

## Registration

| Function | Description |
|----------|-------------|
| `register name` | Register current process under keyword |
| `unregister name` | Remove registration |
| `whereis name` | Look up PID by name |
| `send-named name msg` | Send to registered name |

## Timers

| Function | Description |
|----------|-------------|
| `now` | The scheduler's clock, in ticks |
| `send-after ticks pid msg` | Delayed message delivery |
| `cancel-timer ref` | Cancel a pending timer |

## Process dictionary

| Function | Description |
|----------|-------------|
| `put-dict key val` | Store value, returns old |
| `get-dict key` | Retrieve value |
| `erase-dict key` | Remove key, returns old |

## External API

| Function | Description |
|----------|-------------|
| `process-info sched pid` | Query process state from outside |
| `inject sched pid msg` | Send message from outside scheduler |


---

## See also

- [behaviors.md](behaviors.md) — GenServer, Actor, Task, EventManager
- [supervisor.md](supervisor.md) — Supervisor
- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
- [fibers](signals/fibers.md) — fiber architecture underlying processes
- [runtime.md](runtime.md) — fuel budgets
- [io.md](io.md) — ports
- [subprocess.md](subprocess.md) — spawning and waiting on child processes
- [scheduler.md](scheduler.md) — async event loop
