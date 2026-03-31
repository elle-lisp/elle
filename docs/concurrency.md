# Concurrency

User code runs inside the async scheduler automatically. Fibers are
single-threaded cooperative tasks — concurrent but not parallel. Because
only one fiber runs at a time, shared data requires no synchronization.
For CPU parallelism across cores, see [threads.md](threads.md).

## Spawn and join

```text
# Spawn a fiber, wait for its result
(def f (ev/spawn (fn [] (+ 1 2))))
(ev/join f)                # => 3

# Join a sequence — results in input order
(def [a b] (ev/join [(ev/spawn (fn [] :first))
                      (ev/spawn (fn [] :second))]))
```

## Parallel map

The most common pattern:

```text
(ev/map (fn [url] (http/get url)) urls)
# => [response1 response2 ...]

# Bounded parallelism (at most n in flight)
(ev/map-limited (fn [url] (http/get url)) urls 4)
```

## Error handling

`ev/join-protected` returns `[ok? value]` instead of raising errors:

```text
(let [[[ok? val] (ev/join-protected (ev/spawn (fn [] (flaky-call))))]]
  (if ok? val (fallback)))
```

## Select, race, timeout

```text
# First to complete wins; abort the rest
(ev/race [(ev/spawn (fn [] (query-replica-1)))
          (ev/spawn (fn [] (query-replica-2)))])

# Deadline on a computation
(ev/timeout 5 (fn [] (slow-operation)))

# Wait for first of N — returns [done remaining]
(ev/select fibers)
```

## Scoped concurrency

All children must finish before scope exits. If one child fails, the
others are aborted.

```text
(ev/scope (fn [spawn]
  (let [[users    (spawn (fn [] (fetch "/users")))]
        [settings (spawn (fn [] (fetch "/settings")))]]
    {:users (ev/join users) :settings (ev/join settings)})))
```

## Primitives reference

```text
(ev/spawn thunk)            # create fiber, returns handle
(ev/join fiber-or-seq)      # wait for result(s), propagate errors
(ev/join-protected target)  # wait without raising — [ok? value]
(ev/abort fiber)            # graceful cancel (defer blocks run)
(ev/select fibers)          # wait for first → [done remaining]
(ev/race fibers)            # first wins, abort rest, return value
(ev/timeout secs thunk)     # deadline — value or {:error :timeout}
(ev/scope (fn [spawn] ...)) # nursery — children can't outlive scope
(ev/map f items)            # parallel map, results in order
(ev/map-limited f items n)  # bounded parallel map
(ev/as-completed fibers)    # lazy iterator → [next-fn pool]
(ev/sleep seconds)          # yield for N seconds
```

## TCP

```text
(tcp/listen addr port)      # bind and listen, returns listener
(tcp/accept listener)       # yield until connection, returns port
(tcp/connect host port)     # yield until connected, returns port
```

## Synchronization (lib/sync)

`lib/sync` provides fiber-friendly synchronization primitives built on
`ev/futex-wait` and `ev/futex-wake`. These cooperate with the async
scheduler — waiting fibers yield rather than blocking the thread.

```text
(def sync ((import "lib/sync")))

(def lock (sync:make-lock))
(lock:acquire)
# ... critical section ...
(lock:release)
```

| Primitive | Description |
|-----------|-------------|
| `make-lock` | Mutual exclusion lock |
| `make-semaphore n` | Counting semaphore with `n` permits |
| `make-condvar` | Condition variable (`:wait`, `:notify`, `:broadcast`) |
| `make-rwlock` | Read-write lock (multiple readers or one writer) |
| `make-barrier n` | All `n` fibers must `:wait` before any proceed |
| `make-latch` | One-shot gate — once opened, stays open |
| `make-once thunk` | Lazy one-time initialization; all callers get the cached result |
| `make-queue capacity` | Bounded blocking FIFO queue |


## Processes (lib/process)

`lib/process` provides an Erlang-inspired process model: lightweight
processes with mailboxes, links, monitors, and named registration. Processes
are fibers driven by a cooperative scheduler with fuel-based preemption.

```text
(def process ((import "lib/process")))

(process:start (fn []
  (let [[pid (self)]]
    (send pid :hello)
    (println "received:" (recv)))))
```

| Function | Description |
|----------|-------------|
| `send pid msg` | Send a message to a process mailbox |
| `recv` | Block until a message arrives |
| `recv-match pred` | Block until a message matching `pred` arrives |
| `recv-timeout ticks` | Receive with timeout |
| `self` | Current process PID |
| `spawn fn` | Start a new process |
| `spawn-link fn` | Start linked (crash propagation) |
| `spawn-monitor fn` | Start monitored (death notification) |
| `link pid` / `unlink pid` | Manage crash links |
| `monitor pid` / `demonitor ref` | Manage monitors |
| `register name` | Register current process by name |
| `whereis name` | Look up PID by name |
| `send-named name msg` | Send to a named process |
| `exit pid reason` | Terminate a process |
| `trap-exit flag` | Catch linked exits as messages |


---

## See also

- [fibers.md](fibers.md) — fiber architecture
- [io.md](io.md) — port I/O
- [threads.md](threads.md) — OS threads for CPU parallelism
- [signals.md](signals.md) — signal system
