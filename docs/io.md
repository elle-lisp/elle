# I/O

All I/O in Elle is async — reads and writes yield to the scheduler. User
code runs inside the async scheduler automatically.

## I/O backend

On Linux, Elle uses `io_uring` for all I/O: file reads, writes, TCP,
timers, subprocess pipes. Operations are submitted to the kernel's
submission queue and completed without syscalls or threads — the kernel
handles multiplexing directly. A single-threaded event loop drains any
ready completions, then blocks on the completion queue (waiting for at
least one completion) and resumes the waiting fiber.

On macOS, Elle uses a thread-pool backend that provides the same
abstraction. Blocking I/O operations run on background threads; the
event loop collects results and resumes fibers identically. User code
sees no difference — the same `port/open`, `port/read-line`, `ev/spawn`
API works on both platforms.

Both backends are syscall-free from the fiber's perspective: the fiber
yields `:io`, the scheduler submits the operation, and the fiber resumes
with the result. No threads are created per-operation on Linux; on macOS,
the thread pool is shared across all fibers.

Whatever cannot lift to io_uring — `getaddrinfo`, an arbitrary `Task`
closure, blocking stdin reads, and everything on the thread-pool platform —
runs on a single shared thread pool whose every worker reports through one
completion channel. On the thread-pool platform the scheduler's blocking wait
*is* a `recv()` on that channel. On Linux it blocks on one `io_uring_enter`
instead, and a bridge eventfd carries the channel's completions into it: a
worker raises the eventfd after publishing, and a standing poll on the ring
turns that edge into a completion. Either way the scheduler has exactly one
blocking primitive, so a completion published by a worker can never be missed
while the scheduler is asleep.

### Backend teardown

An io_uring submission queue entry references a buffer the kernel writes
into asynchronously — a `BufferPool` slot for `read-all`/`open`, or the
fiber's own arena buffer for `read`/`read-line`. The kernel may complete
the operation (and write that buffer) at any point up until its completion
is reaped. So a backend must never be torn down while an operation is still
in flight: freeing the buffer pool and the ring with the kernel still
holding a write pointer lands the eventual write in freed heap (manifesting
as a `malloc(): unsorted double linked list corrupted` abort).

Dropping an async backend therefore first brings the ring to a quiescent
state — it cancels every pending io_uring operation and drains the
resulting completions, so no kernel-owned buffer outlives the backend. This
matters when user code submits work it never waits for, e.g. `(io/submit
backend req)` with no following `(io/wait backend …)`: the operation is
in flight when the backend value goes out of scope. Thread-pool and stdin
operations need no such handling — their workers copy results through
channels and never write into a freed pooled buffer.

### Cancelling an operation

`ev/timeout` cancels an operation on every call — the body's or the
timer's, whichever lost — so cancellation runs constantly rather than at
the edges. Two things come back when it does, on either backend:

- **The worker.** A thread-pool operation runs on an OS thread. A
  cancelled operation reports completion like any other, so its thread is
  released; only the result is thrown away. `(ev/report):workers` counts
  the threads currently out.
- **The descriptor.** A cancelled read stops rather than going on
  reading, and so does a cancelled write whose peer stopped taking bytes.
  Whatever arrives next belongs to whoever reads the port next:

  ```text
  (ev/timeout 0.1 (fn [] (port/read p 64)))   # the deadline wins
  (port/read p 64)                            # still sees the peer's bytes
  ```

  That sketch needs a peer slow enough for the deadline to win, so it is
  written out rather than run here; `tests/elle/io-cancel-releases.lisp`
  builds the peer and asserts both lines.

  And a port that goes away while an operation still runs — closed, or
  released with the regions of the fiber that opened it — keeps its
  descriptor number until that operation ends, so the number cannot be
  handed to a new port while a worker holds it.

`tests/elle/io-cancel-releases.lisp` pins both. See `src/io/AGENTS.md`
§ "I/O Cancellation" for how the thread pool delivers them.

### How many operations run at once

However many the OS allows. The thread-pool backend runs each operation
on its own thread, so the ceiling is `RLIMIT_NPROC`, `kernel.threads-max`
and the memory for the stacks; when the OS refuses a thread, `port/read`
and friends signal that refusal rather than the runtime pre-empting it
with a smaller number of its own. io_uring runs its operations in the
kernel and has no such ceiling.

`(io/workers backend)` reports how many worker threads a backend has out,
and `ev/report` carries the running scheduler's count as `:workers`.

## Ports

Ports are bidirectional file descriptors. Open with `port/open`, close
with `port/close`.

```lisp
(with-temp-dir dir
  (let [path (path/join dir "doc-test.txt")]
    (file/write path "hello from elle")
    (def p (port/open path :read))
    (defer (port/close p)
      (port/read-all p))))    # => "hello from elle"
```

### Port operations

```lisp
# (port/open path mode)        — mode: :read, :write, :append, :read-write
# (port/read p n)              — read n bytes
# (port/read-line p)           — read until \n, nil on EOF
# (port/read-all p)            — read everything
# (port/write p data)          — write bytes or string
# (port/flush p)               — flush buffers
# (port/seek p offset)         — seek to byte offset (default: from start)
# (port/tell p)                — current byte position
# (port/close p)               — close port
```

### `port/write` writes every byte

`port/write` returns the length of the data you gave it. The caller never
loops on the return value.

One `write(2)` transfers only what fits in the fd's send buffer at that
moment. On a socket that is often far less than the payload: a 4 KiB send
buffer accepts about 21 KB of a 200 KB write, and a default one accepts a
few megabytes of an 8 MB write. The backend therefore resubmits from the
byte after the last one accepted, and completes the operation only when the
whole payload is gone. `port/read` is the deliberate opposite — it returns
"up to n bytes" per POSIX, and `port/read-exact` is its all-or-nothing
sibling.

If the fd fails part-way through, `port/write` raises the error rather than
returning a short count. An unknown prefix of the payload reached the peer
in that case, the same guarantee `write(2)`-loop helpers give elsewhere.

The pinning tests are `tests/elle/port-shortwrite.lisp` and
`tests/elle/port-shortread-framing.lisp` for the read direction.

### A read that overshoots keeps the rest for the same port

`port/read-line` stops at the newline and `port/read-exact` stops at the Nth
grapheme, but the kernel read behind each of them takes a whole block. The
backend holds the extra bytes and serves the next read on that port from them
before it goes back to the kernel, so no byte is lost between two reads and
`port/tell` reports the logical position rather than the kernel offset.

The remainder belongs to the port that produced it, not to its descriptor
*number*. A closed descriptor's number goes straight back to the OS — and a
port dropped without `port/close` still closes its descriptor — so the next
`port/open` can be handed that number. It starts with an empty remainder,
whichever port held the number before it. The pinning test is
`tests/elle/io.lisp` § "a recycled descriptor number carries no remainder".

What a read reserves before it runs is not a bound on what it answers with.
`port/read-line` reserves 64 KiB, which covers every real protocol line; a line
longer than that is answered in pieces, and reading on gives the next piece
until the newline arrives. No byte is dropped to make an answer fit — the
backend has already taken those bytes from the kernel, so there would be
nothing left to read them again. `tests/elle/port-longline.lisp` pins it, and
`port_longline_threadpool` (`tests/integration/elle_scripts.rs`) pins it on the
other backend.

On a text port `port/read-exact` counts grapheme clusters, and a cluster has no
upper bound in bytes: one emoji built from four people and three joiners is 25
bytes and one grapheme. So the byte length of `n` clusters is not known until
the bytes arrive. The backend reads in chunks, holds what it has, and sizes the
result to what it turns out to be — the count in the request bounds the answer
in clusters and says nothing about its bytes. A `read-exact` that follows an
over-reading `read-line` is the same story: the held remainder joins the bytes
this read produces, the first `n` clusters of the join are the answer, and the
rest goes back to the remainder for the next read on that port.
`tests/elle/port-text-framing.lisp` pins all three, and
`port_text_framing_threadpool` (`tests/integration/elle_scripts.rs`) pins them
on the other backend.

### `:timeout` bounds each operation

Every port call takes an optional `:timeout` in milliseconds, and it bounds
each kernel operation rather than the whole call.

Most calls are a single operation, so the two readings agree. They part
company on the calls that loop — `port/write` until the payload is gone,
`port/read-exact` until its count, `port/read-all` until EOF, and
`port/read-line` until a newline. There, a peer that has stopped trips the
deadline, while a peer that is merely slow keeps making progress and the call
finishes however long that takes.

```lisp
(defn listener-port [listener]
  "The port number a listener bound to an ephemeral port received."
  (let [path (port/path listener)]
    (parse-int (slice path (+ 1 (string/find path ":"))))))

(defn with-quiet-peer [body]
  "Run `body` against a peer that accepts the connection and then neither
   reads nor writes. A small send buffer makes it stall the writer quickly."
  (ev/run (fn []
            (let [listener (tcp/listen "127.0.0.1" 0)]
              (ev/spawn (fn []
                          (let [conn (tcp/accept listener)]
                            (ev/sleep 2)
                            (port/close conn))))
              (let [conn (tcp/connect "127.0.0.1" (listener-port listener)
                                      :sndbuf 4096 :timeout 5000)]
                (defer (begin (port/close conn) (port/close listener))
                  (body conn)))))))

(defn timed-out? [thunk]
  "True when `thunk` signals the :timeout error."
  (let [[ok? err] (protect (thunk))]
    (and (not ok?) (= (get err :error) :timeout))))

# The peer is connected and sends nothing, so the read gives up at its deadline.
(assert (with-quiet-peer (fn [conn]
                           (timed-out? (fn [] (port/read-line conn :timeout 200)))))
        "a peer that sends nothing must trip the read's deadline")

# The peer never reads, so the send buffer fills and the write gives up too.
(assert (with-quiet-peer
          (fn [conn]
            (timed-out? (fn []
                          (port/write conn (bytes (string/repeat "x" 2000000))
                                      :timeout 200)))))
        "a peer that never reads must trip the write's deadline")
```

A peer that is merely slow is the opposite case, and it is what the
per-operation reading buys. Every gap here stays inside the deadline while the
whole call runs well past it:

```lisp
(ev/run (fn []
          (let* [listener (tcp/listen "127.0.0.1" 0)
                 chunk 4096
                 chunks 20
                 gap 0.05
                 deadline 500]
            (ev/spawn (fn []
                        (let [conn (tcp/accept listener)]
                          (repeat chunks
                                  (port/write conn (bytes (string/repeat "z" chunk)))
                                  (ev/sleep gap))
                          (port/close conn))))
            (let* [conn (tcp/connect "127.0.0.1" (listener-port listener)
                                     :timeout 5000)
                   want (* chunk chunks)
                   started (clock/monotonic)
                   got (port/read-exact conn want :timeout deadline)
                   elapsed (- (clock/monotonic) started)]
              (port/close conn)
              (port/close listener)
              (assert (= (length got) want)
                      "a slow peer still delivers every byte")
              # The read returns once the last chunk lands, after `chunks - 1`
              # gaps — so this bound sits between the deadline and that total,
              # and the gap sits an order of magnitude under the deadline. Tie
              # either margin closer and a loaded scheduler's overshoot on one
              # `ev/sleep` trips the deadline this example exists to survive.
              (assert (> elapsed 0.5)
                      "and the call outlives its own :timeout while doing it")))))
```

Both readings stop a hang; only the per-operation one keeps a slow transfer
working. `port/read` is unaffected either way: it is a single "up to n bytes"
operation.

The bound covers every kind of port. A socket peer that stops reading, a child
process that never reads its stdin, and a fifo nobody opens for reading all
stall the same way, and `:timeout` returns from all three.

```lisp
# The child never reads its stdin, so the pipe buffer fills and the rest of
# the payload has nowhere to go.
(ev/run (fn []
          (let [child (subprocess/exec "sleep" ["30"])]
            (assert (timed-out? (fn []
                                  (port/write (get child :stdin)
                                              (bytes (string/repeat "x" 1000000))
                                              :timeout 200)))
                    "a child that never reads must trip the write's deadline")
            (subprocess/kill child :sigterm)
            (subprocess/wait child))))
```

The pinning tests are `tests/elle/port-write-timeout.lisp` and
`tests/elle/port-read-timeout.lisp`, both run on each backend, each covering a
socket peer and a pipe peer.

### The calls that wait for a peer

`tcp/accept`, `unix/accept`, `tcp/connect`, `unix/connect` and
`udp/recv-from` take the same `:timeout`, and they need it most: each waits
on a peer that may never appear. A listener nobody calls, a datagram socket
nobody sends to, and a connect to an address that drops the packet all wait
alike.

```lisp
(ev/run (fn []
          (let [listener (tcp/listen "127.0.0.1" 0)]
            (assert (timed-out? (fn [] (tcp/accept listener :timeout 200)))
                    "an accept nobody calls must trip its own deadline")
            (port/close listener))))
```

`ev/timeout` and `io/cancel` end these calls too, on either backend. The
pinning tests are `tests/elle/net-wait-timeout.lisp` for the deadline and
the `a_cancelled_pool_*` tests in `src/io/aio/tests/net.rs` for the
cancellation.

### The calls that wait for something other than a peer

Four more calls wait on something that may never happen, and each takes the
same two endings — its own `:timeout` where it has one, and `ev/timeout` or
`io/cancel` from outside.

| Call | Waits for | Ends on |
|---|---|---|
| `port/open` on a fifo for writing | a reader opening the other end | `:timeout`, cancel |
| `subprocess/wait` | the child exiting | cancel |
| `watch-next` | a filesystem event | cancel |
| `os/sig-next` | a signal arriving | cancel |
| `ev/poll-fd` | the descriptor becoming ready | its `timeout` argument, cancel |

`ev/poll-fd` answers an expired wait with `0` rather than signalling, which is
what lets a caller poll in a loop — `lib/wayland.lisp` polls with a 33 ms bound
on every iteration.

```lisp
(ev/run (fn []
          (let [child (subprocess/exec "sleep" ["30"])]
            (assert (nil? (ev/timeout 0.2 (fn [] (subprocess/wait child))))
                    "a child that outlives the deadline must not hold the wait")
            (subprocess/kill child :sigkill)
            (subprocess/wait child))))
```

`port/open` is the one with a direction to it. POSIX blocks an `open(2)` on a
fifo until the other end is open, and Elle keeps that for the write side: the
open waits for a reader, and `:timeout` bounds the wait. On the read side the
port comes back at once and the first `port/read` is what waits for a writer,
which is where a reader's `:timeout` applies anyway.

Each of these is pinned in `tests/elle/io-cancel-releases.lisp`, which measures
what a cancelled operation gives back, and in `src/io/aio/tests/park.rs`.

### Streams from ports

```lisp
# (port/lines p)               — lazy stream of lines
# (port/chunks p n)            — lazy stream of byte chunks
# (port/writer p)              — writable stream
```

### Closing `*stdin*`

`(port/close *stdin*)` is supported and does what you'd expect:

- Any in-flight `(port/read-line *stdin*)` / `(port/read …)` /
  `(port/read-all *stdin*)` is cancelled. The waiting fiber resumes
  with an `:io-error` whose message is `stdin closed`.
- The dedicated stdin worker thread (which sits on `read(2)` against
  fd 0) is signalled via an internal self-pipe, returns from its
  syscall, drains any further pending requests as cancelled, and
  exits cleanly. No leaked OS thread.
- Subsequent stdin reads error from the `port is closed` check
  in `AsyncBackend::submit`.
- The OS file descriptor for stdin is *not* itself closed (the
  stdio ports never owned it). This matches the existing
  `*stdout*` / `*stderr*` close semantics.

`port/close` on `*stdin*` is idempotent.

## Output

```lisp
(print "no newline")
(println "with newline")
(println "count: " 42)         # multiple args concatenated
(eprint "to stderr")
(eprintln "error: bad input")
(pp {:a [1 2 3]})              # pretty-print data structures
```

All output functions are async — they yield to the scheduler.
`*stdout*` and `*stderr*` are dynamic parameters that can be rebound.

## Subprocesses

### Run to completion

`subprocess/system` runs a command and captures its output:

```lisp
# Run to completion — returns {:exit :stdout :stderr}
(subprocess/system "echo" ["hello"])
# => {:exit 0 :stdout "hello\n" :stderr ""}

# With options
(subprocess/system "ls" ["-la"] {:cwd "/usr"})
(subprocess/system "env" [] {:env {:FOO "bar"}})
```

### Long-running subprocesses

`subprocess/exec` spawns a subprocess and returns a handle with stdio
ports. Use `subprocess/wait` to block until exit, `subprocess/kill` to
send signals.

```lisp
# Spawn and interact
(def proc (subprocess/exec "cat" []))
(port/write (get proc :stdin) "hello")
(port/close (get proc :stdin))
(string (port/read-all (get proc :stdout)))  # => "hello"
(subprocess/wait proc)                       # => 0

# Spawn, kill, reap
(def proc (subprocess/exec "sleep" ["60"]))
(subprocess/kill proc :sigterm)
(subprocess/wait proc)                       # => non-zero
```

### Subprocess options

```lisp
# (subprocess/exec program args)           — default: pipes for all stdio
# (subprocess/exec program args opts)      — with options struct
#
# Options:
#   :env    — struct of env vars (merged with inherited)
#   :cwd    — working directory string
#   :stdin  — :pipe (default) | :null | :inherit
#   :stdout — :pipe (default) | :null | :inherit
#   :stderr — :pipe (default) | :null | :inherit
```

### Supervised subprocesses

For long-running daemons, use `lib/process` to supervise OS subprocesses.
The supervisor automatically restarts them on crash:

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:supervisor-start-link
    [(process:make-subprocess-child :worker "/usr/bin/worker" []
       :opts {:env {:PORT "8080"}})
     (process:make-subprocess-child :monitor "/usr/bin/monitor" [])]
    :name :daemon-sup
    :max-restarts 5)))
```

See [processes.md](processes.md) for the full supervisor API.

## Temporary files

`file/mktempdir` creates a uniquely-named directory under the platform
temp root and returns its path. The root is the runtime's native temp
location — `TMPDIR` on Unix (the per-user folder on macOS), `%TEMP%` on
Windows — so scripts never hardcode `/tmp`; point `TMPDIR` at a tmpfs
such as `/dev/shm` to keep scratch I/O in RAM. Uniqueness is made in
the runtime (pid + counter, retried on collision), so concurrent
processes cannot race each other to the same name the way fixed
scratch filenames do.

`with-temp-dir` scopes one to a body and deletes it on the way out —
recursively, error or not. `file/delete-dir-all` is the underlying
recursive delete (`file/delete-dir` only removes empty directories).

```lisp
(with-temp-dir dir
  (file/write (path/join dir "scratch.txt") "data")
  (assert (= (file/read (path/join dir "scratch.txt")) "data")))
```

## System args and environment

```lisp
# sys/args returns args after the source file
(def args (sys/args))

# Environment
(sys/env)              # => struct of all env vars
(sys/env "HOME")       # => single var, or nil
```

---

## See also

- [processes.md](processes.md) — supervised subprocesses, GenServer, actors
- [concurrency.md](concurrency.md) — ev/spawn, ev/join, parallel I/O
- [fibers](signals/fibers.md) — fiber-based async model
- [strings.md](strings.md) — string operations
