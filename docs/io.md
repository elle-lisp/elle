# I/O

<!-- audited: 2026-10-21 -->

All I/O in Elle is async: each call yields to the scheduler, and other fibers
run until it completes. User code runs inside the async scheduler
automatically. A call that cannot block runs on the scheduler's thread and
returns at once. The `file/` functions that only read or change a file's
metadata are of this kind; the ones that move a file's contents are not.
[Files](io/files.md) lists both.

## I/O backend

A Linux build with the `uring` feature uses `io_uring` for port reads and
writes, TCP, timers and subprocess pipes. Operations are submitted to the
kernel's submission queue and completed without syscalls or threads — the
kernel handles multiplexing directly. A single-threaded event loop drains the
ready completions, then blocks on the completion queue until one arrives, and
resumes the waiting fiber.

Every other build ([config](config.md)) uses a thread-pool backend that
provides the same abstraction. Blocking I/O operations run on background
threads; the event loop collects results and resumes fibers identically. User
code sees no difference: `port/open`, `port/read-line` and `ev/spawn` work the
same on both backends.

Whatever cannot lift to io_uring — `getaddrinfo`, an arbitrary `Task`
closure, blocking stdin reads, and everything on the thread-pool backend —
runs on a single shared thread pool whose every worker reports through one
completion channel. On the thread-pool backend the scheduler's blocking wait
*is* a `recv()` on that channel. On io_uring it blocks on one `io_uring_enter`
instead, and a bridge eventfd carries the channel's completions into it: a
worker raises the eventfd after publishing, and a standing poll on the ring
turns that edge into a completion. Either way the scheduler has exactly one
blocking primitive, so a completion published by a worker can never be missed
while the scheduler is asleep.

### Backend teardown

An io_uring submission queue entry references memory the kernel reaches
asynchronously: a `BufferPool` slot for `read-all`, the fiber's own region
buffer for `read`, `read-line` and `read-exact`, and the payload's region pages
for `write`. The kernel may complete the operation, and write or read that
memory, at any point up until its completion is reaped. So a backend must never
be torn down while an operation is still in flight: freeing that memory with
the kernel still holding a pointer lands the eventual write in freed heap
(manifesting as a `malloc(): unsorted double linked list corrupted` abort).

Dropping an async backend therefore first brings the ring to a quiescent
state — it cancels every pending io_uring operation and drains the
resulting completions, so no kernel-owned buffer outlives the backend. This
matters when user code submits work it never waits for, as in `(io/submit
backend req)` with no following `(io/wait backend …)`: the operation is
in flight when the backend value goes out of scope.

A thread-pool worker reads into the fiber's buffer and writes from the
payload's pages the same way, so the drop also stops every pool operation that
carries a stop pipe and waits for its completion. The stdin worker reads into a
buffer of its own and hands it over through the channel, so it needs no wait.
[Where a stream operation's bytes live](impl/io-bytes.md) holds the argument.

A backend the program never lets go of — a top-level `(io/backend :async)`,
or the scheduler's own — is still there when the heap that carries it is torn
down. The heap brings each one to a quiescent state before it frees its
regions, so the drain reads values that are still allocated and the
operations still in flight let go of what they hold while there is something
to let go of. Without that order the backend's own drop would run inside the
teardown sweep, after the sweep had freed part of what it holds.

### Cancelling an operation

`ev/timeout` cancels an operation on every call — the body's or the
timer's, whichever lost — so cancellation runs constantly rather than at
the edges. Two things come back when it does, on either backend:

- **The worker.** A thread-pool operation runs on a worker thread. A
  cancelled operation reports completion like any other, so its worker
  goes back to the pool for the next operation; only the result is thrown
  away. `(ev/report):workers` counts the operations out right now.
- **The descriptor.** A cancelled read stops rather than going on
  reading, and so does a cancelled write whose peer stopped taking bytes.
  Whatever arrives next belongs to whoever reads the port next. Here the
  peer is a shell that waits 0.3 s before it writes:

  ```lisp
  (let* [peer (subprocess/exec "/bin/sh" ["-c" "sleep 0.3; printf late"])
         out (get peer :stdout)]
    (assert (nil? (ev/timeout 0.1 (fn [] (port/read out 64))))
            "the deadline wins, and ev/timeout answers nil")
    (assert (= (bytes "late") (port/read out 64))
            "the next read still sees the peer's bytes")
    (subprocess/wait peer))
  ```

  And a port that goes away while an operation still runs — closed, or
  released with the regions of the fiber that opened it — keeps its
  descriptor number until that operation ends, so the number cannot be
  handed to a new port while a worker holds it.

[io-cancel-releases.lisp](../tests/lang/io-cancel-releases.lisp) pins the
descriptor, and [io-cancel-workers.lisp](../tests/impl/io-cancel-workers.lisp)
counts the workers. See
[an operation in flight](impl/io-inflight.md) for how a cancelled operation is
ended, and [descriptors and workers](impl/io-descriptor.md) for what it gives
back.

### How many operations run at once

However many the OS allows. The thread-pool backend runs each operation
on a worker thread and starts one whenever no worker is free, so the
ceiling is `RLIMIT_NPROC`, `kernel.threads-max` and the memory for the
stacks; when the OS refuses a thread, `port/read` and friends signal that
refusal rather than the runtime pre-empting it with a smaller number of
its own. io_uring runs its operations in the kernel and has no such
ceiling.

A worker that finishes an operation waits for the next one instead of
exiting, and retires after ten idle seconds. So a program that keeps
asking for I/O pays for a thread once rather than per operation, and one
that stops asking stops paying.

`*io-keepalive*` is that wait, in seconds. A scheduler reads it when it
makes its backend, so bind it around the `ev/run` whose I/O it should
govern; `0` turns reuse off, and every operation then starts and ends a
thread of its own.

```lisp
(assert (= :done (parameterize ((*io-keepalive* 0))
                    (ev/run (fn [] (ev/sleep 0) :done))))
        "bind *io-keepalive* around the ev/run whose backend it governs")
```

`(io/workers backend)` reports how many operations a backend has out —
the workers busy right now, not the ones waiting for work — and
`ev/report` carries the running scheduler's count as `:workers`.

## Ports

Ports are bidirectional file descriptors. Open with `port/open`, close
with `port/close`.

### Port operations

| Call | Does |
|------|------|
| `(port/open path mode)` | open a file; `mode` is `:read`, `:write`, `:append` or `:read-write` |
| `(port/read p n)` | read up to `n` bytes; `nil` at EOF |
| `(port/read-exact p n)` | read exactly `n` units; `nil` if EOF comes first |
| `(port/read-line p)` | read to the next newline, without it, whatever the line's length; `nil` at EOF |
| `(port/read-all p)` | read everything that remains |
| `(port/write p data)` | write all of a string or bytes |
| `(port/flush p)` | flush a file port's data to stable storage, as `fsync(2)` does; nothing on any other port |
| `(port/seek p offset)` | seek to a byte offset, from the start unless `:from :current` or `:from :end` |
| `(port/tell p)` | the current logical byte position |
| `(port/close p)` | close the port; idempotent |

```lisp
(with-temp-dir dir
  (let [path (path/join dir "lines.txt")]
    (let [w (port/open path :write)]
      (port/write w "one\ntwo\n")
      (port/close w))
    (let [r (port/open path :read)]
      (assert (= "one" (port/read-line r)) "read-line drops the newline")
      (assert (= 4 (port/tell r)) "tell counts what was consumed, not the kernel offset")
      (assert (= "two" (port/read-line r)))
      (assert (nil? (port/read-line r)) "nil at EOF")
      (assert (= 0 (port/seek r 0)) "seek answers the new position")
      (assert (= "one" (port/read-line r)) "and reads from there")
      (assert (= "two\n" (port/read-all r)) "read-all takes the rest")
      (port/close r)
      (port/close r))))                  # closing twice is harmless
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

The pinning tests are [port-shortwrite.lisp](../tests/lang/port-shortwrite.lisp) and
[port-shortread-framing.lisp](../tests/lang/port-shortread-framing.lisp) for the read direction.

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
whichever port held the number before it. [io.lisp](../tests/lang/io.lisp)
pins it.

What a read reserves before it runs is not a bound on what it answers with.
`port/read-line` reserves 64 KiB, and a line that fits is answered from that
buffer in place. A longer line is read on until its newline arrives, and
answered whole: one call, one line, however long. A caller never sees a piece
of a line, so a newline-delimited protocol with no bound on its line length,
JSON-RPC over a socket for instance, reads each message with one call. No byte
is dropped to make an answer fit — the backend has already taken those bytes
from the kernel, so there would be nothing left to read them again.
[port-longline.lisp](../tests/lang/port-longline.lisp) pins it on each backend.

```lisp
(with-temp-dir dir
  (let [path (path/join dir "long.txt")
        long (string/repeat "x" 70000)]
    (file/write path (string long "\nnext\n"))
    (let [r (port/open path :read)]
      (assert (= long (port/read-line r)) "a line past 64 KiB is one answer")
      (assert (= "next" (port/read-line r)) "and the read after it is the next line")
      (port/close r))))
```

A program with records of its own framing, lines and then a body by its length,
cuts them from one chunk of a binary port with [framing](../lib/framing.md).
Each record is a view of the chunk.

On a text port `port/read-exact` counts grapheme clusters, and a cluster has no
upper bound in bytes: one emoji built from four people and three joiners is 25
bytes and one grapheme. So the byte length of `n` clusters is not known until
the bytes arrive. The backend reads in chunks, holds what it has, and sizes the
result to what it turns out to be — the count in the request bounds the answer
in clusters and says nothing about its bytes. A `read-exact` that follows an
over-reading `read-line` is the same story: the held remainder joins the bytes
this read produces, the first `n` clusters of the join are the answer, and the
rest goes back to the remainder for the next read on that port.
[port-text-framing.lisp](../tests/lang/port-text-framing.lisp) pins all three on
each backend.

### Deadlines

Port calls take a `:timeout` and a `:deadline`, both in seconds, and so do the
calls that wait on a peer. [I/O deadlines](io/timeout.md) owns what each bound
covers, and how a call waiting on a child, a signal or a filesystem event ends.

### Streams from ports

A stream is a fiber. Each of these closes the port when the stream ends.

| Call | Stream |
|------|--------|
| `(port/lines p)` | yields each line, without its newline |
| `(port/chunks p n)` | yields reads of up to `n` units |
| `(port/writer p)` | writes each value it is resumed with; resume with `nil` to close |

```lisp
(with-temp-dir dir
  (let [path (path/join dir "stream.txt")
        w (port/writer (port/open path :write))]
    (fiber/resume w)                     # run it to its first wait
    (each s ["one\n" "two\n" nil] (fiber/resume w s))   # nil closes the port
    (assert (= (list "one" "two") (stream/collect (port/lines (port/open path :read))))
            "the lines come back without their newlines")))
```

### Closing `*stdin*`

`(port/close *stdin*)` cancels every read of stdin in flight, and each waiting
fiber resumes with an `:io-error` whose message is `stdin closed`. The stdin
worker thread, which waits in `read(2)` on fd 0, wakes through an internal
self-pipe, cancels the requests still queued, and exits. A later read of stdin
raises from the `port is closed` check in `AsyncBackend::submit`. Descriptor 0
itself stays open, because the stdio ports never owned it, as with `*stdout*`
and `*stderr*`.

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

`subprocess/exec` spawns a child and answers a `subprocess` — the value its
streams, pid and exit status are read from, and the value `subprocess/wait` and
`subprocess/kill` take. Its stdio ports are ordinary ports, so everything above
applies to them: the same reads and writes, the same bounds, the same
cancellation.

See [subprocess.md](subprocess.md) for the type, its reads, and what a kill
answers.

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

- [io/timeout.md](io/timeout.md) — the `:timeout` and `:deadline` bounds, and how a waiting call ends
- [io/files.md](io/files.md) — the `file/` functions by path, temporary directories and `file/sync`
- [subprocess.md](subprocess.md) — the subprocess type, its reads, kill and wait
- [behaviors.md](behaviors.md) — supervised subprocesses, GenServer, actors
- [concurrency.md](concurrency.md) — ev/spawn, ev/join, parallel I/O
- [fibers](signals/fibers.md) — fiber-based async model
- [strings.md](strings.md) — string operations
