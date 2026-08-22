# Threads

OS threads for CPU-bound work. For I/O-bound concurrency, prefer
`ev/spawn` / `ev/join` (see [concurrency.md](concurrency.md)).

## spawn and join

```lisp
(def handle (sys/spawn (fn [] (+ 1 2))))
(sys/join handle)          # => 3
(sys/thread-id)            # current OS thread ID
```

`sys/spawn` **deep-copies** the closure and all captured values into the
new thread via `SendValue`. The threads share nothing — mutations on one
side are invisible to the other. Values that cannot be serialized (fibers,
open ports) will error at spawn time. The thread's result comes back the
same way: it is serialized into a shared slot and reconstructed in the
joining thread's heap.

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

```text
# Light worker — special forms + primitives + intrinsics resolve; stdlib does not.
(sys/join (sys/spawn-vm (fn [] (eval '(%add 1 2)))))           # => 3
(sys/join (sys/spawn-vm (fn [] (eval '(begin (def x 7) x)))))  # => 7  (begin/def are special forms)
(sys/join (sys/spawn-vm (fn [] (eval '(+ 1 2)))))              # error: Unknown symbol (+ is stdlib)

# Heavy worker — stdlib is loaded, so eval resolves the full vocabulary.
(sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3])))))   # => [2 3 4]
```

#### Quoted symbols cross the boundary by name

A deep-copied value (closure, quoted datum, channel message) crosses threads via
`SendValue`. A worker's symbol table is its own — interned IDs are not comparable
across threads — so any **symbol value** in the copied data is serialized with its
*name* and **re-interned** into the receiving thread's table (exactly as keywords
are). This is what lets `(eval '(begin …))` work in a worker: the analyzer matches
special forms by name, and the name survives the copy even though `begin`'s ID in
the worker's table differs from the sender's. (Symbol references baked into
compiled *bytecode* travel separately, via the closure template's `symbol_names`
map; this paragraph is about symbols that appear as runtime *data*.)

Aliases: `sys/spawn` = `os/spawn` (heavy); `sys/spawn-vm` = `os/spawn-vm` (light).
There is no bare `spawn` — it was an ambiguous global (it collides with the
`(ev/scope (fn [spawn] …))` nursery param), so it is not a primitive alias;
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

clamped to a sane `[2 MB, 64 MB]` range. The earlier symptom — only a stack of
64 MB "fixing" large-file compiles — was a misdiagnosis: the real worst-case
corpus file needs only ~3–4 MB of worker stack in a debug build; matching the
main thread gives ample headroom without reserving an absurd per-worker stack.

#### A worker owns its heap and gives it back

A worker builds a whole instance of its own: a VM, a symbol table, a compile
context, and the region heap all its values live in. The result crosses back to
the joiner as a serialized bundle — a deep copy — so nothing the worker
allocated is reachable once the thread ends. The worker therefore **owns** its
heap: the thread's exit tears every region down and returns the pages to the
OS.

This is what bounds a program that runs many workers in sequence. The test
runner is the extreme case: it ships each corpus file to its own worker, twice
(once per JIT policy), so a worker heap that outlived its thread would make the
runner's memory the sum of every file it has run, and a batch of 25 files peaked
at 13 GB before the fix. A worker's cost is now its own, and a run's peak is the
largest single file rather than the whole batch.

`tests/integration/thread_transfer/heap.rs` pins the bound with the mapped-page
gauge (`elle::value::fiberheap::mapped_bytes`), which counts the bytes the
region page pools hold from the OS across every thread.

`sys/spawn` does **not** start a scheduler — wrap the body in `(ev/run …)`
yourself if it does async I/O (`ev/run` resolves there, since stdlib is
loaded). Neither does it import the test file's own top-level `def`s into the
worker — `eval` compiles in a global scope and cannot see the spawning
closure's lexical upvalues. Prefer `sys/spawn-vm` whenever the worker doesn't
need stdlib at runtime; `init_stdlib` per spawn is not free.

### join with a deadline

```lisp
(sys/join handle)          # wait indefinitely, return the result
(sys/join handle 5000)     # wait at most 5000 ms
```

`sys/join` (aliases `os/join`, `join`) **cooperates with the scheduler**:
it does not poll and it does not park the OS thread. While it waits, other
fibers on the same scheduler continue to run. It is built on the same
cross-thread wake path as `chan/select` — when the worker finishes it
signals a completion channel, waking any parked joiner exactly once
(see [concurrency.md](concurrency.md) § Synchronization and the
`chan/select` cross-thread wake protocol).

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

> A scheduler must be present at the join site (it always is for
> top-level code and inside `ev/run`). Joining a still-running thread
> from a context with no scheduler raises a yield error — give that
> context a scheduler by running under `ev/run`.

## Channels

Crossbeam-based channels for inter-fiber and inter-thread messaging.

```lisp
(def [tx rx] (chan))           # unbounded channel

(chan/send tx 42)              # => [:ok]
(chan/recv rx)                 # => [:ok 42]

(chan/clone tx)                # clone sender (multiple producers)
(chan/close tx)                # close sender half
(chan/close-recv rx)           # close receiver half
```

---

## See also

- [concurrency.md](concurrency.md) — async concurrency with ev/spawn
- [fibers](signals/fibers.md) — fiber architecture
