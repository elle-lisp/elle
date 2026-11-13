# I/O

<!-- audited: 2026-11-12 -->

All I/O in Elle is async: each call yields to the scheduler, and other fibers
run until it completes. User code runs inside the async scheduler
automatically. A call that cannot block runs on the scheduler's thread and
returns at once. The `file/` functions that only read or change a file's
metadata are of this kind; the ones that move a file's contents are not.
[Files](io/files.md) lists both.

## I/O backend

A Linux build with the `uring` feature hands port reads and writes, sockets,
timers and subprocess pipes to the kernel through `io_uring`. Every other build
([config](config.md)) runs them as blocking calls on a thread pool. On either
backend, a single-threaded event loop resumes each fiber when its operation
completes. User code sees no difference: `port/open`, `port/read-line` and
`ev/spawn` work the same on both. [The platforms](impl/io/platforms.md) say
which work runs where, and how the loop waits.

A backend that goes away with operations in flight cancels them first. It
waits for each one that addresses a buffer or a payload, so no kernel or worker
writes into freed memory. [Where a stream operation's bytes
live](impl/io/bytes.md) holds the argument.

### Cancelling an operation

`ev/timeout` cancels an operation on every call: the body's or the timer's,
whichever lost. So cancellation runs constantly rather than at the edges. Two
things come back when it does, on either backend:

- **The worker.** A thread-pool operation runs on a worker thread. A
  cancelled operation reports completion like any other, so its worker goes
  back to the pool for the next operation. Only the result is thrown away.
- **The descriptor.** A cancelled read stops rather than going on reading,
  and so does a cancelled write whose peer stopped taking bytes. Whatever
  arrives next belongs to whoever reads the port next. Here the peer is a
  shell that waits 0.3 s before it writes:

  ```lisp
  (let* [peer (subprocess/exec "/bin/sh" ["-c" "sleep 0.3; printf late"])
         out (get peer :stdout)]
    (assert (nil? (ev/timeout 0.1 (fn [] (port/read out 64))))
            "the deadline wins, and ev/timeout answers nil")
    (assert (= (bytes "late") (port/read out 64))
            "the next read still sees the peer's bytes")
    (subprocess/wait peer))
  ```

  A port can go away while an operation still runs, closed or released with
  the regions of the fiber that opened it. It keeps its descriptor number
  until that operation ends, so the number cannot go to a new port while a
  worker holds it.

[io-cancel-releases.lisp](../tests/lang/io-cancel-releases.lisp) pins the
descriptor, and [io-cancel-workers.lisp](../tests/impl/io-cancel-workers.lisp)
counts the workers. [Cancelling an operation](impl/io/cancel.md) says how a
cancelled operation ends, and [descriptors and
workers](impl/io/descriptor.md) what it gives back.

### How many operations run at once

However many the OS allows. Nothing in the runtime caps the count. When the OS
refuses a thread, `port/read` and the other calls raise that refusal. A worker
that finishes an operation waits for the next one instead of exiting, and
retires after ten idle seconds. So a program that keeps asking for I/O pays for
a thread once rather than per operation. `*io-keepalive*` sets that wait
([dynamic parameters](parameters.md)), and [descriptors and
workers](impl/io/descriptor.md) says why nothing caps the count.

## Ports

A port is a file descriptor with a direction, an encoding and a kind. Open one
with `port/open`, and close it with `port/close`.

### Port operations

| Call | Does |
|------|------|
| `(port/open path mode)` | open a file; `mode` is `:read`, `:write`, `:append` or `:read-write` |
| `(port/read p n)` | read up to `n` bytes; `nil` at EOF |
| `(port/read-exact p n)` | read exactly `n` units; `nil` if EOF comes first |
| `(port/read-line p)` | read to the next newline, without it, whatever the line's length; `nil` at EOF |
| `(port/read-all p)` | read everything that remains |
| `(port/write p data)` | write all of a string or bytes |
| `(port/write-many p parts)` | write every part of a list or array of strings and bytes, in order, as one write |
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

[Writing to a port](io/write.md) holds what `port/write` and
`port/write-many` promise: every byte, in order, sent from where it lies.

### A read that overshoots keeps the rest for the same port

`port/read-line` stops at the newline and `port/read-exact` stops at the Nth
grapheme, but the kernel read behind each of them takes a whole block. The
backend holds the extra bytes and serves the next read on that port from them
first. So no byte is lost between two reads, and `port/tell` reports the
logical position rather than the kernel offset.

The remainder belongs to the port that produced it, not to its descriptor
*number*. A closed descriptor's number goes straight back to the OS, and a port
dropped without `port/close` still closes its descriptor. So the next
`port/open` can be handed that number, and it starts with an empty remainder.
[io.lisp](../tests/lang/io.lisp) pins it.

`port/read-line` answers one whole line, however long. A caller never sees a
piece of a line, so a newline-delimited protocol with no bound on its line
length, JSON-RPC over a socket for instance, reads each message with one call.
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
cuts them from one chunk of a binary port with [framing](stdlib/framing.md).
Each record is a view of the chunk.

On a text port `port/read-exact` counts grapheme clusters, and a cluster has no
upper bound in bytes. One emoji built from four people and three joiners is 25
bytes and one grapheme. So the count in the request bounds the answer in
clusters and says nothing about its bytes. A `read-exact` that follows an
over-reading `read-line` takes the held bytes first, and keeps what it does not
answer with for the next read on that port.
[port-text-framing.lisp](../tests/lang/port-text-framing.lisp) pins both on
each backend, and [where a stream operation's bytes live](impl/io/bytes.md)
says how a read sizes its answer.

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
fiber resumes with an `:io-error` whose message is `stdin closed`. The thread
that reads descriptor 0 exits. A later read of stdin raises `port is closed`.
Descriptor 0 itself stays open, because the stdio ports never owned it, as
with `*stdout*` and `*stderr*`.

## Output

```lisp
(print "no newline")
(println "with newline")
(println "count: " 42)         # multiple args concatenated
(eprint "to stderr")
(eprintln "error: bad input")
(pp {:a [1 2 3]})              # pretty-print data structures
```

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

## See also

- [behaviors.md](behaviors.md) — supervised subprocesses, GenServer, actors
- [concurrency.md](concurrency.md) — ev/spawn, ev/join, parallel I/O
- [fibers](signals/fibers.md) — fiber-based async model
- [strings.md](strings.md) — string operations
