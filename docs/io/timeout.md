# I/O deadlines

<!-- audited: 2026-09-28 -->

How long a port call, an accept, a connect or a wait may take, and how each one
ends when nothing arrives.

The [I/O](../io.md) document owns the ports these calls operate on.

## `:timeout` bounds each operation

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

The pinning tests are [port-write-timeout.lisp](../../tests/lang/port-write-timeout.lisp) and
[port-read-timeout.lisp](../../tests/lang/port-read-timeout.lisp), each covering a socket
peer and a pipe peer. The language suite runs every build, so each backend runs
all of these ([testing](../testing.md)).

## The calls that wait for a peer

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
pinning tests are [net-wait-timeout.lisp](../../tests/lang/net-wait-timeout.lisp) for the deadline and
the `a_cancelled_pool_*` tests in [netcancel.rs](../../src/io/aio/tests/netcancel.rs) for the
cancellation.

## The calls that wait for something other than a peer

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
what lets a caller poll in a loop — [wayland.lisp](../../lib/wayland.lisp) polls with a 33 ms bound
on every iteration.

A cancelled `subprocess/wait` is the one that gives something back besides the
fiber: it may have reaped the child on its way out, and a reap takes the exit
status from the kernel for good. [subprocess.md](../subprocess.md) owns what
becomes of that status.

`port/open` is the one with a direction to it. POSIX blocks an `open(2)` on a
fifo until the other end is open, and Elle keeps that for the write side: the
open waits for a reader, and `:timeout` bounds the wait. On the read side the
port comes back at once and the first `port/read` is what waits for a writer,
which is where a reader's `:timeout` applies anyway.

[io-cancel-releases.lisp](../../tests/lang/io-cancel-releases.lisp) pins the child
wait and the fifo open. [park.rs](../../src/io/aio/tests/park.rs) pins the pool's child
wait, fifo open, `watch-next` and `ev/poll-fd`, and
[signals.rs](../../src/io/threadpool/tests/signals.rs) its `os/sig-next`.

