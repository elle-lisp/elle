# Processes

<!-- audited: 2026-09-16 -->

`lib/process.lisp` provides Erlang-style concurrent processes built on
Elle's fiber scheduler. Processes have mailboxes, links, monitors, named
registration, and fuel-based preemption.

The callback-driven roles built on that model — GenServer, Actor, Task,
Supervisor and EventManager — are in [behaviors.md](behaviors.md).

## Loading

```lisp
(def process ((import "std/process")))
```

## Starting a process system

`process:start` creates a scheduler and runs a closure as the first
process. It blocks until all processes complete and returns the scheduler.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (println "hello from process 0")))
```

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

## Links and crash propagation

Linked processes crash together. When a linked child crashes, the parent
crashes too — unless the parent is trapping exits.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:trap-exit true)
  (let [child (process:spawn-link (fn []
                 (error {:error :boom :message "crash"})))]
    (match (process:recv)
      [:EXIT pid reason]
        (begin
          (assert (= pid child) "EXIT from child")
          (match reason
            [:error _] (assert true "got error reason")
            _ nil))
      _ nil))))
```

## Monitors

Monitors deliver a `[:DOWN ref pid reason]` message when the monitored
process dies, without affecting the monitoring process.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [[child-pid ref] (process:spawn-monitor (fn [] :done))]
    (match (process:recv)
      [:DOWN got-ref got-pid reason]
        (begin
          (assert (= got-ref ref) "correct ref")
          (match reason
            [:normal val] (assert (= val :done) "normal exit")
            _ nil))
      _ nil))))
```

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
sub-fibers alike. The scheduler dispatches a process fiber's wait
through one path and a sub-fiber's through another, and both implement
the full wait vocabulary; a wait op neither knows is a protocol error,
raised in the fiber that emitted it rather than swallowed.

One rule keeps those waits honest: the scheduler never resumes a parked
fiber except with the result it parked for. A join or select on a
hand-built `:new` fiber gives that fiber a first run; a `:paused` fiber
is already parked on I/O, a futex, or a wait the scheduler tracks, and
is woken only by its own completion. Resuming it out of turn would hand
its park a nil — a timer parked in `ev/sleep` would "complete"
instantly, and every `ev/timeout` in a process would report its
deadline at once. Pinned by `tests/elle/process-select.lisp`.

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
| `make-scheduler` | Create scheduler (`:fuel`, `:backend`) |
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
| `monitor pid` | Monitor another process |
| `demonitor ref` | Remove monitor |
| `trap-exit flag` | Catch linked exits as messages |
| `exit pid reason` | Terminate a process |

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

- [behaviors.md](behaviors.md) — GenServer, Actor, Task, Supervisor, EventManager
- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
- [fibers](signals/fibers.md) — fiber architecture underlying processes
- [runtime.md](runtime.md) — fuel budgets
- [io.md](io.md) — ports
- [subprocess.md](subprocess.md) — spawning and waiting on child processes
- [scheduler.md](scheduler.md) — async event loop
