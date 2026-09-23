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
A process can block in a receive, on a futex, or in a join or select on a
sub-fiber. When no process or sub-fiber can run, and no timer or I/O is
pending, nothing can wake a blocked process, and every live process is idle
for good. `process:start` then exits each of them with reason `:shutdown` and
returns. While PID 0 itself is one of them, nothing can end the program, and
`process:start` raises `{:error :deadlock}`.

```lisp
(def process ((import "std/process")))

## PID 0 returns while the process it linked waits in recv.
(process:start (fn []
  (process:spawn-link (fn [] (process:recv)))
  :done))

## PID 0 parks on a futex that no other process can wake.
(let [[ok? err] (protect (process:start (fn [] (ev/futex-wait :k (box 0) 0))))]
  (assert (not ok?) "a park that nothing can wake ends the scheduler")
  (assert (= (get err :error) :deadlock) "as a deadlock"))
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
a process scheduler forwards its I/O to its parent scheduler (see
[process-scheduler.md](process-scheduler.md)).

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
every ready process once, adds one tick. `now` returns the current tick, and
`recv-timeout`, `send-after` and a supervisor's `:max-ticks` all count it.
When no process is ready, the scheduler waits, and the clock counts the wait:

- With no I/O in flight, only a timer can end the wait. The clock jumps to the
  earliest timer.
- With I/O in flight, the wait ends when a completion arrives or when the
  earliest timer falls due, whichever comes first. The clock advances one tick
  for each whole millisecond of the wait.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [before (process:now)]
    (process:recv-timeout 10)
    (assert (>= (- (process:now) before) 10) "recv-timeout waited 10 ticks"))))
```

A timer therefore fires while another process waits on long I/O:

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [sleeper (process:spawn (fn [] (ev/sleep 30)))
        started (clock/monotonic)]
    (assert (= (process:recv-timeout 5) :timeout) "the timer fires")
    (assert (< (- (clock/monotonic) started) 10) "long before the sleep ends")
    (process:exit sleeper :kill))))
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

## API reference

### Core

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

### Links and monitors

| Function | Description |
|----------|-------------|
| `link pid` | Link to another process |
| `unlink pid` | Remove link |
| `monitor pid` | Monitor another process, returns a ref |
| `demonitor ref` | Remove monitor (`:flush`) |
| `trap-exit flag` | Catch linked exits as messages |
| `exit pid reason` | Send an exit signal to a process |

### Registration

| Function | Description |
|----------|-------------|
| `register name` | Register current process under keyword |
| `unregister name` | Remove registration |
| `whereis name` | Look up PID by name |
| `send-named name msg` | Send to registered name |

### Timers

| Function | Description |
|----------|-------------|
| `now` | The scheduler's clock, in ticks |
| `send-after ticks pid msg` | Delayed message delivery |
| `cancel-timer ref` | Cancel a pending timer |

### Process dictionary

| Function | Description |
|----------|-------------|
| `put-dict key val` | Store value, returns old |
| `get-dict key` | Retrieve value |
| `erase-dict key` | Remove key, returns old |

### External API

| Function | Description |
|----------|-------------|
| `process-info sched pid` | Query process state from outside |
| `inject sched pid msg` | Send message from outside scheduler |

## See also

- [process-scheduler.md](process-scheduler.md) — sub-fibers, forwarded I/O and nested schedulers
- [behaviors.md](behaviors.md) — GenServer, Actor, Task, EventManager
- [supervisor.md](supervisor.md) — Supervisor
- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
- [fibers](signals/fibers.md) — fiber architecture underlying processes
- [runtime.md](runtime.md) — fuel budgets
- [io.md](io.md) — ports
- [subprocess.md](subprocess.md) — spawning and waiting on child processes
- [scheduler.md](scheduler.md) — async event loop
