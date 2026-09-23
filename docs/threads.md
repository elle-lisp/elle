# Threads

<!-- audited: 2026-09-23 -->

OS threads for CPU-bound work. For I/O-bound concurrency, prefer
`ev/spawn` / `ev/join` (see [concurrency.md](concurrency.md)).

## spawn and join

```lisp
(def handle (sys/spawn (fn [] (+ 1 2))))
(assert (= 3 (sys/join handle)) "join answers the worker's result")
(assert (integer? (sys/thread-id)) "the current OS thread ID")
```

`sys/spawn` **deep-copies** the closure and all captured values into the
new thread as a `SendBundle`. The threads share nothing — mutations on one
side are invisible to the other. A value that cannot be serialized (a fiber,
an open file or socket port) makes the spawn fail with a `:thread-error`. The
thread's result comes back the same way: it is serialized into a shared slot
and reconstructed in the joining thread's heap.

```lisp
(def a-fiber (fiber/new (fn [] 1) 0))
(let [[ok? err] (protect (sys/spawn (fn [] a-fiber)))]
  (assert (not ok?) "a fiber cannot cross to another thread")
  (assert (= (get err :error) :thread-error) "the spawn says why"))
```

A worker does inherit one thing: the spawning fiber's withheld
capabilities. A thread cannot reach what the fiber that spawned it could
not — see [capabilities](signals/capabilities.md) for what a denial in a
worker does, since a worker has no parent to mediate it.

### Two worker environments: `sys/spawn` vs `sys/spawn-vm`

Both run a deep-copied closure on a fresh OS thread with its own VM; they
differ in how much of the language is *materialized* in that worker, which
matters for **runtime reflection** (`eval`, `read`, `meta`) — code the worker
compiles on the fly, which resolves names against the worker's own symbol
table and globals (the shipped closure itself doesn't need this — its
references are baked in at compile time and ride along in the bundle).

| Primitive | Worker has | `eval` can resolve | Cost |
|-----------|-----------|--------------------|------|
| `sys/spawn-vm` | primitives + `%`-intrinsics | special forms, primitives, intrinsics | cheap |
| `sys/spawn` | the above **+ the standard library** | special forms, primitives, intrinsics, **stdlib** (`+`, `map`, …) | `init_stdlib` per spawn |

**Special forms** (`if`, `fn`, `begin`/`do`, `def`/`var`, `let`, `quote`, …) are
recognized by the analyzer *by name*, so they resolve in **either** worker — they
are not stdlib and need no materialization. What distinguishes the two workers is
only whether stdlib *functions* (`+`, `map`, …) are present.

```lisp
# Light worker — special forms + primitives + intrinsics resolve; stdlib does not.
(assert (= 3 (sys/join (sys/spawn-vm (fn [] (eval '(%add 1 2)))))))
(assert (= 7 (sys/join (sys/spawn-vm (fn [] (eval '(begin (def x 7) x)))))))
(let [[ok? err] (protect (sys/join (sys/spawn-vm (fn [] (eval '(+ 1 2))))))]
  (assert (not ok?) "+ is stdlib, so the light worker cannot resolve it")
  (assert (= (get err :error) :thread-error)))

# Heavy worker — stdlib is loaded, so eval resolves the full vocabulary.
(assert (= [2 3 4] (sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3])))))))
```

#### Quoted symbols cross the boundary unchanged

A deep-copied value (closure, quoted datum, channel message) crosses threads
inside a `SendBundle`. A symbol id is the FNV-1a hash of its name
([impl/symbol.md](impl/symbol.md)), so it means the same thing on both sides and
copies verbatim — as an immediate, with no name and no re-interning. This is what
lets `(eval '(begin …))` work in a worker: `begin`'s id in the worker is
`begin`'s id everywhere. The same holds for symbols baked into compiled
*bytecode*, which needs no name table to travel.

Aliases: `sys/spawn` = `os/spawn` (heavy); `sys/spawn-vm` = `os/spawn-vm` (light).
There is no bare `spawn` — it would collide with the
`(ev/scope (fn [spawn] …))` nursery param, so it is not a primitive alias;
top-level code uses `sys/spawn`/`sys/spawn-vm`.

#### Worker stack size matches the main thread

A worker must be able to compile and run anything the main thread can. The test
runner ships a file's *syntax* to a worker, which compiles it with the worker's
own stdlib — and the frontend's HIR passes (notably `functionalize`) recurse
depth-first over the program, so a deep file needs as much stack as the main
thread or the worker overflows mid-compile (a raw `SIGSEGV` — a stack overflow on
a secondary thread blows past the guard page with no panic message).

Rust's `std::thread::spawn` gives a new thread only a **2 MB** default stack,
versus the main thread's `RLIMIT_STACK` (commonly 8 MB). Workers are therefore
spawned with an explicit stack sized to the main thread's limit. Resolution
order:

1. `RUST_MIN_STACK`, if set — the same override `std` itself honors, so it works
   for workers too;
2. otherwise the main thread's `RLIMIT_STACK` soft limit;
3. otherwise an 8 MB fallback (when the limit is unbounded or unreadable),

clamped to a `[2 MB, 64 MB]` range. The deepest corpus file needs about 3–4 MB
of worker stack in a debug build, so matching the main thread leaves ample
headroom without reserving an absurd per-worker stack.

#### A worker owns its heap and gives it back

A worker builds a whole instance of its own: a VM, a symbol table, a compile
context, and the region heap all its values live in. The result crosses back to
the joiner as a serialized bundle — a deep copy — so nothing the worker
allocated is reachable once the thread ends. The worker therefore **owns** its
heap: the thread's exit tears every region down and returns the pages to the
OS.

This is what bounds a program that runs many workers in sequence. The test
runner is the extreme case: it ships each corpus file to its own worker, twice
(once per JIT policy). A worker heap that outlived its thread would make the
runner's memory the sum of every file it has run; instead a run's peak is the
largest single file.

[worker_heap.rs](../tests/worker_heap.rs) pins the bound with the mapped-page
gauge (`elle::value::fiberheap::mapped_bytes`), which counts the bytes the
region page pools hold from the OS across every thread.

`sys/spawn` does **not** start a scheduler — wrap the body in `(ev/run …)`
yourself if it does async I/O (`ev/run` resolves there, since stdlib is
loaded). An I/O call outside one ends the worker with a `:thread-error`.
`eval` in a worker compiles in the worker's global scope, so it cannot see
the spawning closure's lexical upvalues. Prefer `sys/spawn-vm` whenever the
worker doesn't need stdlib at runtime; `init_stdlib` per spawn is not free.

```lisp
(let [[ok? err] (protect (sys/join (sys/spawn (fn [] (ev/sleep 0) :slept))))]
  (assert (not ok?) "a worker has no scheduler of its own")
  (assert (= (get err :error) :thread-error)))
(assert (= :slept (sys/join (sys/spawn (fn [] (ev/run (fn [] (ev/sleep 0) :slept))))))
        "ev/run gives it one")
```

### join with a deadline

`(sys/join handle)` waits indefinitely and returns the result;
`(sys/join handle 5000)` waits at most 5000 ms.

`sys/join` (alias `os/join`) **cooperates with the scheduler**:
it does not poll and it does not park the OS thread. While it waits, other
fibers on the same scheduler continue to run. It is built on the same
cross-thread wake path as `chan/select` — when the worker finishes it
signals a completion channel, waking any parked joiner exactly once;
`(doc sys/join)` describes the protocol.

- With no `timeout-ms`, `sys/join` waits until the thread completes.
- With `timeout-ms` (a non-negative integer of milliseconds), if the
  thread has not finished by the deadline, `sys/join` raises a typed
  timeout error — the struct `{:error :timeout :message ...}` — which
  `protect` catches as `[false {:error :timeout ...}]`. The worker is
  **not** cancelled: there is no safe way to kill a running OS thread, so
  a timed-out worker is abandoned (it runs to completion on its own and
  its result is discarded). Each worker has its own VM and shares nothing,
  so an abandoned worker cannot corrupt the joiner.
- A worker that vanishes without producing a result (an unwinding panic
  in the thread) surfaces as `{:error :thread-error ...}`.

`sys/join` is idempotent: once a thread has completed, repeated joins
return the same result without waiting.

```lisp
(def busy (sys/spawn-vm (fn []
  (let [@i 0]
    (while (%lt i 5000000) (assign i (%add i 1)))
    i))))
(let [[ok? err] (protect (sys/join busy 1))]
  (assert (not ok?) "the deadline passes first")
  (assert (= (get err :error) :timeout)))
(assert (= 5000000 (sys/join busy)) "a later join still gets the result")
(assert (= 5000000 (sys/join busy)) "and so does every join after it")
```

> A scheduler must be present at the join site (it always is for
> top-level code and inside `ev/run`). Joining a still-running thread
> from a context with no scheduler raises a yield error — give that
> context a scheduler by running under `ev/run`.

## Channels

Crossbeam-based channels for inter-fiber and inter-thread messaging.
`chan/send` and `chan/recv` do not block: a send answers `[:ok]`, `[:full]`
or `[:disconnected]`, and a receive answers `[:ok msg]`, `[:empty]` or
`[:disconnected]`.

```lisp
(def [tx rx] (chan))                     # unbounded channel

(assert (= [:ok] (chan/send tx 42)))
(assert (= [:ok 42] (chan/recv rx)))

(def tx2 (chan/clone tx))                # clone sender (multiple producers)
(chan/close tx)                          # close one sender
(chan/send tx2 1)
(assert (= [:ok 1] (chan/recv rx)) "the clone still delivers")
(chan/close tx2)                         # close the last sender
(assert (= [:disconnected] (chan/recv rx)))
(chan/close-recv rx)                     # close receiver half
```

## See also

- [concurrency.md](concurrency.md) — async concurrency with ev/spawn
- [fibers](signals/fibers.md) — fiber architecture
