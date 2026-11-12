# gather

<!-- audited: 2026-11-11 -->

A write queue over a port or a sink: each write queues a value where it lies, and a flush sends the queue in one gathered write.

The export struct at the bottom of [gather.lisp](gather.lisp) lists the three
calls, and `doc` on an export bound to a name carries its contract.
[framing](framing.md) is the read side's counterpart.

A protocol often writes a message as several values: a status line, each
header, a blank line, a body. Joining them into one value before the write
copies the body, and writing each one as it comes parks the fiber once per
value. A gather does neither. `write` queues the value itself, and `flush`
hands the whole queue to one `port/write-many`, which sends every part from
the pages it lies in ([writing to a port](../docs/io/write.md)).

```lisp
(def gather ((import "std/gather")))

(with-temp-dir dir
  (let [path (path/join dir "message")
        body (bytes "hello")]
    (let [p (port/open-bytes path :write)
          g (gather:open p)]
      (gather:write g "HTTP/1.1 200 OK\r\n")
      (gather:write g "content-length: 5\r\n\r\n")
      (gather:write g body)
      (assert (= 0 (length (file/read path))) "a write sends nothing")
      (gather:flush g)
      (port/close p))
    (assert (= "HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello" (file/read path))
            "the flush sends every write, in order")))
```

## The sink

`gather:open` takes a port or a function of one argument. A port receives each
flush as one `port/write-many`. A function receives the flush's parts as an
array, and sends them as it can: a TLS connection encrypts every byte into a
record, which copies it anyway, so its sink joins the parts and makes one
record of them.

```lisp
(def sent @[])
(def g (gather:open (fn [parts] (push sent parts))))
(gather:write g "a")
(gather:write g (bytes "b"))
(gather:flush g)
(gather:flush g)
(assert (= [["a" (bytes "b")]] (freeze sent))
        "one call per flush that has something to send")
```

## What a write queues

A write takes a string or bytes. An immutable value is queued as it is, and
nothing copies it. A mutable `@string` or `@bytes` is copied once when it is
written, so the flush sends the bytes the write was given, whatever the program
does to the store afterwards. An empty value queues nothing. Any other value
raises a `gather-error` whose `:reason` is `:not-bytes`, and queues nothing.

```lisp
(def got @[])
(def g (gather:open (fn [parts] (push got parts))))
(def buf @b[1 2])
(gather:write g buf)
(put buf 0 9)
(gather:write g "")
(gather:flush g)
(assert (= [[(bytes 1 2)]] (freeze got)) "a mutable value is sent as it was written")
(let [[ok? err] (protect (gather:write g 42))]
  (assert (and (not ok?) (= :not-bytes (get err :reason)))
          "only strings and bytes are written"))
```

## Two fibers

A flush takes the queue before it sends it, and the gather starts a new queue.
The send parks the fiber, and another fiber can write and flush while it waits.
Its values go into the new queue, so they leave in a later send, and no flush
sends a value another flush already took.
[transport-writes.lisp](../tests/lang/transport-writes.lisp) holds the HTTP
and HTTP/2 transports to it.

The order in which two fibers' sends reach the descriptor is the scheduler's,
not the gather's.

## Who writes through one

The TCP and TLS transports of [http](http.md), [http2](http2.md) and
`std/websocket` each write through one gather over their connection. A caller
that writes through a transport writes and then flushes, and the message
leaves in one write.
