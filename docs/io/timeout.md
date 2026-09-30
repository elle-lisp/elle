# I/O deadlines

<!-- audited: 2026-09-30 -->

How long a port call, an accept, a connect or a wait may take, and how each one
ends when nothing arrives.

The [I/O](../io.md) document owns the ports these calls operate on.

## Two bounds, both in seconds

Every call that waits takes two optional named arguments:

- `:timeout` is a duration in seconds, an integer or a float. It bounds each
  wait the call makes.
- `:deadline` is a reading of `(clock/monotonic)`: the seconds since its first
  reading in this process. It bounds the whole call, however many waits it
  makes.

A call that names neither waits as long as it takes, and `nil` names neither.
A bound further off than the clock can count is refused with an
`:argument-error`. A call may name both, and then each wait ends at whichever
comes first. A deadline that has already passed allows no wait at
all: the call answers what is ready now, or it ends as a timed-out call ends.
The clock is one clock for the whole process, so a deadline read on one thread
bounds a call on another.

```lisp
(defn timed-out? [thunk]
  "True when `thunk` signals the :timeout error."
  (let [[ok? err] (protect (thunk))]
    (and (not ok?) (= (get err :error) :timeout))))

(let [[tx rx] (chan)
      start (clock/monotonic)]
  (assert (= (chan/select @[rx] :timeout 0.05) [:timeout])
          "a select nothing answers ends at its timeout")
  (assert (>= (- (clock/monotonic) start) 0.05)
          "and never before the timeout has passed")
  (let [until (+ (clock/monotonic) 0.05)]
    (assert (= (chan/select @[rx] :deadline until) [:timeout])
            "a deadline ends it the same way")
    (assert (>= (clock/monotonic) until) "and never before the deadline")))
```

A file that declares epoch 13 or earlier writes these bounds in milliseconds,
as positional arguments where the call took one. The compiler migrates each to
`:timeout` in seconds ([epochs](../epochs.md)).

## `:timeout` bounds each operation

A port call's `:timeout` bounds each kernel operation rather than the whole
call.

Most calls are a single operation, so the two readings agree. They part
company on the calls that loop — `port/write` until the payload is gone,
`port/read-exact` until its count, `port/read-all` until EOF, and
`port/read-line` until a newline. There, a peer that has stopped trips the
timeout, while a peer that is merely slow keeps making progress and the call
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
                                      :sndbuf 4096 :timeout 5)]
                (defer (begin (port/close conn) (port/close listener))
                  (body conn)))))))

# The peer is connected and sends nothing, so the read gives up at its timeout.
(assert (with-quiet-peer (fn [conn]
                           (timed-out? (fn [] (port/read-line conn :timeout 0.2)))))
        "a peer that sends nothing must trip the read's timeout")

# The peer never reads, so the send buffer fills and the write gives up too.
(assert (with-quiet-peer
          (fn [conn]
            (timed-out? (fn []
                          (port/write conn (bytes (string/repeat "x" 2000000))
                                      :timeout 0.2)))))
        "a peer that never reads must trip the write's timeout")
```

A peer that is merely slow is the opposite case, and it is what the
per-operation reading buys. Every gap here stays inside the timeout while the
whole call runs well past it. The same peer is what separates the two bounds:
a deadline shorter than the transfer ends the same read.

```lisp
(defn with-slow-peer [body]
  "Run `body` against a peer that sends twenty 4 KiB chunks 50 ms apart,
   with the byte count it will deliver."
  (ev/run (fn []
            (let [listener (tcp/listen "127.0.0.1" 0)]
              (ev/spawn (fn []
                          (let [conn (tcp/accept listener)]
                            (repeat 20
                                    (port/write conn (bytes (string/repeat "z" 4096)))
                                    (ev/sleep 0.05))
                            (port/close conn))))
              (let [conn (tcp/connect "127.0.0.1" (listener-port listener)
                                      :timeout 5)]
                (defer (begin (port/close conn) (port/close listener))
                  (body conn (* 4096 20))))))))

(with-slow-peer
  (fn [conn want]
    (let* [started (clock/monotonic)
           got (port/read-exact conn want :timeout 0.5)
           elapsed (- (clock/monotonic) started)]
      (assert (= (length got) want) "a slow peer still delivers every byte")
      # The read returns once the last chunk lands, after nineteen gaps — so
      # this bound sits between the timeout and that total, and the gap sits
      # an order of magnitude under the timeout. Tie either margin closer and
      # a loaded scheduler's overshoot on one `ev/sleep` trips the timeout
      # this example exists to survive.
      (assert (> elapsed 0.5)
              "and the call outlives its own :timeout while doing it"))))

(with-slow-peer
  (fn [conn want]
    (assert (timed-out? (fn []
                          (port/read-exact conn want
                                           :deadline (+ (clock/monotonic) 0.3))))
            "a deadline ends the transfer that a timeout lets finish")))
```

Only the per-operation bound keeps a slow transfer working, and only the
deadline caps what the transfer may cost. `port/read` is a single "up to n
bytes" operation, so both bounds end it at the same point.

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
                                              :timeout 0.2)))
                    "a child that never reads must trip the write's timeout")
            (subprocess/kill child :sigterm)
            (subprocess/wait child))))
```

`port/set-options` gives a port a `:timeout` of its own. Each call on that port
that names no `:timeout` takes the port's, and `:timeout nil` removes it again.

```lisp
(ev/run (fn []
          (let [child (subprocess/exec "sleep" ["30"])
                out (get child :stdout)]
            (port/set-options out :timeout 0.2)
            (assert (timed-out? (fn [] (port/read out 64)))
                    "a read that names no bound takes the port's")
            (subprocess/kill child :sigterm)
            (subprocess/wait child))))
```

The pinning tests are [port-write-timeout.lisp](../../tests/lang/port-write-timeout.lisp) and
[port-read-timeout.lisp](../../tests/lang/port-read-timeout.lisp), each covering a socket
peer and a pipe peer, and [port-deadline.lisp](../../tests/lang/port-deadline.lisp) for the
deadline and the port's own timeout. The language suite runs every build, so each
backend runs all of these ([testing](../testing.md)).

## The calls that wait for a peer

`tcp/accept`, `unix/accept`, `tcp/connect`, `unix/connect` and
`udp/recv-from` take the same two bounds, and they need them most: each waits
on a peer that may never appear. A listener nobody calls, a datagram socket
nobody sends to, and a connect to an address that drops the packet all wait
alike.

```lisp
(ev/run (fn []
          (let [listener (tcp/listen "127.0.0.1" 0)]
            (assert (timed-out? (fn [] (tcp/accept listener :timeout 0.2)))
                    "an accept nobody calls must trip its own timeout")
            (port/close listener))))
```

`ev/timeout` and `io/cancel` end these calls too, on either backend. The
pinning tests are [net-wait-timeout.lisp](../../tests/lang/net-wait-timeout.lisp) for the bounds and
the `a_cancelled_pool_*` tests in [netcancel.rs](../../src/io/aio/tests/netcancel.rs) for the
cancellation.

## The calls that wait for something other than a peer

Five more calls wait on something that may never happen, and each takes the
same two endings — its own bounds where it has them, and `ev/timeout` or
`io/cancel` from outside.

| Call | Waits for | Ends on |
|---|---|---|
| `port/open` on a fifo for writing | a reader opening the other end | `:timeout`, `:deadline`, cancel |
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
open waits for a reader, and the bounds limit the wait. On the read side the
port comes back at once and the first `port/read` is what waits for a writer,
which is where a reader's bounds apply anyway.

[io-cancel-releases.lisp](../../tests/lang/io-cancel-releases.lisp) pins the child
wait and the fifo open. [park.rs](../../src/io/aio/tests/park.rs) pins the pool's child
wait, fifo open, `watch-next` and `ev/poll-fd`, and
[signals.rs](../../src/io/threadpool/tests/signals.rs) its `os/sig-next`.

## The waits that are not port calls

The same two bounds end the waits a program makes on other threads, on
channels and on the scheduler itself. Each is one wait, so `:timeout` and
`:deadline` differ only in how the caller writes the end.

| Call | Waits for | When the bound passes |
|---|---|---|
| `sys/join`, `os/join` | a thread's result | signals `{:error :timeout}` |
| `chan/select` | a message on any receiver | answers `[:timeout]` |
| `chan/wait-ready` | a receiver becoming ready | answers `nil`, as any wake does |
| `io/wait` | a backend's completions | answers the completions so far, maybe none |
| `ev/step` | one round of the event loop | answers `:pending` |

`io/wait` and `ev/step` with `:timeout 0` poll. `ev/shutdown` takes the same
bounds for a different wait: how long aborted fibers may unwind before the loop
cancels them. Without one they get no time at all.

[threads](../threads.md) owns the join, and [chan-select-deadline.lisp](../../tests/lang/chan-select-deadline.lisp)
pins the select and the join against the clock.
