# Concurrency

<!-- audited: 2026-09-23 -->

User code runs inside the async scheduler automatically. Fibers are
single-threaded cooperative tasks — concurrent but not parallel. Because
only one fiber runs at a time, shared data requires no synchronization.
For CPU parallelism across cores, see [threads.md](threads.md).

## Spawn and join

```lisp
# Spawn a fiber, wait for its result
(def f (ev/spawn (fn [] (+ 1 2))))
(ev/join f)                # => 3

# Join a sequence — results in input order
(def [a b] (ev/join [(ev/spawn (fn [] :first))
                      (ev/spawn (fn [] :second))]))
```

## Parallel map

The most common pattern:

```lisp
(ev/map (fn [x] (* x x)) [1 2 3 4])   # => [1 4 9 16]

# Bounded parallelism (at most n in flight)
(ev/map-limited (fn [x] (* x x)) [1 2 3 4] 2)
```

## Error handling

`ev/join-protected` returns `[ok? value]` instead of raising errors:

```lisp
(let [[ok? val] (ev/join-protected (ev/spawn (fn [] (+ 1 2))))]
  (if ok? val :failed))   # => 3
```

## Select, race, timeout

```lisp
# First to complete wins; abort the rest
(ev/race [(ev/spawn (fn [] :fast))
          (ev/spawn (fn [] (ev/sleep 1) :slow))])  # => :fast

# Deadline on a computation
(ev/timeout 5 (fn [] (+ 1 2)))   # => 3
```

## Scoped concurrency

All children must finish before scope exits. If one child fails, the
others are aborted.

```lisp
(ev/scope (fn [spawn]
  (let [a (spawn (fn [] :users))
        b (spawn (fn [] :settings))]
    {:users (ev/join a) :settings (ev/join b)})))
```

## Primitives reference

```lisp
# (ev/spawn thunk)            — create fiber, returns handle
# (ev/join fiber-or-seq)      — wait for result(s), propagate errors
# (ev/join-protected target)  — wait without raising: [ok? value]
# (ev/abort fiber)            — graceful cancel (defer blocks run)
# (ev/select fibers)          — wait for first: [done remaining]
# (ev/race fibers)            — first wins, abort rest, return value
# (ev/timeout secs thunk)     — deadline: value or nil on timeout
# (ev/scope (fn [spawn] ...)) — nursery: children can't outlive scope
# (ev/map f items)            — parallel map, results in order
# (ev/map-limited f items n)  — bounded parallel map
# (ev/as-completed fibers)    — lazy iterator: [next-fn pool]
# (ev/sleep seconds)          — yield for N seconds
```

## TCP

```lisp
# (tcp/listen addr port)      — bind and listen, returns listener
# (tcp/accept listener)       — yield until connection, returns port
# (tcp/connect host port)     — yield until connected, returns port
```

## Synchronization (lib/sync)

`lib/sync` provides fiber-friendly synchronization primitives built on
`ev/futex-wait` and `ev/futex-wake`. These cooperate with the async
scheduler — waiting fibers yield rather than blocking the thread.

```lisp
(def sync ((import "std/sync")))

(def lock (sync:make-lock))
(lock:acquire)
# ... critical section ...
(lock:release)
```

| Primitive | Description |
|-----------|-------------|
| `make-futex initial` | The box with park and wake that the others are built on |
| `make-lock` | Mutual exclusion lock |
| `make-semaphore n` | Counting semaphore with `n` permits |
| `make-condvar` | Condition variable (`:wait`, `:notify`, `:broadcast`) |
| `make-rwlock` | Read-write lock (multiple readers or one writer) |
| `make-barrier n` | All `n` fibers must `:wait` before any proceed |
| `make-latch` | One-shot gate — once opened, stays open |
| `make-once thunk` | Lazy one-time initialization; all callers get the cached result |
| `make-queue capacity` | Bounded blocking FIFO queue |
| `make-monitor` | A lock and a condition variable together (`:with`, `:wait`, `:notify`, `:broadcast`) |


## Processes (lib/process)

`lib/process` provides an Erlang/OTP-inspired process model: lightweight
processes with mailboxes, links, monitors, named registration, and
fuel-based preemption. Built entirely on fibers and signals.

On top of the core process API, the module provides:

- **GenServer** — callback-based servers with call/cast/info dispatch
- **Actor** — simple state wrapper over GenServer
- **Supervisor** — automatic child restart with strategies and intensity limits
- **Task** — one-shot async work as a monitored process
- **EventManager** — pub/sub event dispatching

```lisp
(def process ((import "std/process")))

(process:start (fn []
  # Ping-pong between two processes
  (let* [me (process:self)
         peer (process:spawn (fn []
                 (match (process:recv)
                   [from :ping] (process:send from :pong)
                   _ nil)))]
    (process:send peer [me :ping])
    (assert (= (process:recv) :pong) "pong received"))))
```

### GenServer example

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] 0)
     :handle-call (fn [req _from state]
       (case req
         :inc [:reply (+ state 1) (+ state 1)]
         :get [:reply state state]))}
    nil :name :counter)
  (process:gen-server-call :counter :inc)
  (process:gen-server-call :counter :inc)
  (assert (= 2 (process:gen-server-call :counter :get)) "counter is 2")))
```

### Supervisor example

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:supervisor-start-link
      [{:id :worker :restart :permanent
        :start (fn []
          (process:send me [:started (process:self)])
          (forever (process:recv)))}]
      :name :sup
      :max-restarts 3)
    (match (process:recv)
      [:started pid] (assert (integer? pid) "worker started")
      _ nil))))
```

See [processes.md](processes.md) for the core API — mailboxes, links, monitors,
registration, and structured concurrency inside processes. See
[behaviors.md](behaviors.md) for GenServer, Actor, Task and EventManager, and
[supervisor.md](supervisor.md) for restart strategies, supervisor logging, and
supervised subprocesses.

---

## See also

- [processes.md](processes.md) — full process API
- [behaviors.md](behaviors.md) — GenServer, Actor, Task, EventManager
- [supervisor.md](supervisor.md) — Supervisor
- [fibers](signals/fibers.md) — fiber architecture
- [io.md](io.md) — port I/O
- [subprocess.md](subprocess.md) — spawning and waiting on child processes
- [threads.md](threads.md) — OS threads for CPU parallelism
- [signals](signals/index.md) — signal system
