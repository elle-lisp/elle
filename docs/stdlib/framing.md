# framing

<!-- audited: 2026-11-12 -->

A record cursor over any byte source, and a write queue that sends a message's parts in one gathered write.

The export struct at the bottom of [framing.lisp](../../lib/framing.lisp) lists the
twelve calls, and `doc` on an export bound to a name carries its contract. The
write queue's four live in [gather.lisp](../../lib/framing/gather.lisp).

```lisp
(def framing ((import "std/framing")))
(def {:line line} framing)
(assert (string? (doc line)) "each export carries its docstring")
```

A stream in Elle is a fiber, and the stdlib's `stream/map`, `stream/take` and
`stream/collect` transform one whatever it yields ([io](../io.md)).
Framing sits below that layer, on the byte side. A framer takes bytes in chunks
from a source, cuts records out of them, and answers the records one at a time
or as a stream. A gather queues the values of a message, and sends them in one
write.

## The source

`framing:open` takes any of four sources and asks it for `:size` bytes at a
time, 65536 unless given. It never closes the source.

- A binary port. A text port is refused, because `port/read-exact` counts
  graphemes there and the framer counts bytes.
- A function of one argument, answering up to that many bytes and `nil` at
  the end. A TLS connection is this source: `(fn [n] (tls:read conn n))`.
- A fiber, each resume answering one chunk, ending as `stream/collect` reads
  an end. `port/chunks` and `tls:chunks` answer such a fiber, and so does any
  generator.
- Bytes or a string already in memory: one chunk, then the end.

A chunk is bytes or a string; a string or mutable bytes is copied once, and
immutable bytes are taken as they are. A source answers at least one byte or
`nil`: an empty answer is not an end.

```lisp
(defn lines-of [source]
  "The lines a framer over source answers, as strings."
  (def out @[])
  (def f (framing:open source :size 4))
  (def @l (framing:line f))
  (while (not (nil? l))
    (push out (string l))
    (assign l (framing:line f)))
  (freeze out))

(def text "one\r\ntwo\nthree")
(def expected ["one" "two" "three"])
(assert (= expected (lines-of (bytes text))) "bytes in memory are one chunk")
(assert (= expected (lines-of text)) "and so is a string")
(with-temp-dir dir
  (let [path (path/join dir "text")]
    (file/write path text)
    (let [p (port/open-bytes path :read)]
      (assert (= expected (lines-of p)) "a binary port, read four bytes at a time")
      (port/close p))
    (assert (= expected (lines-of (port/chunks (port/open-bytes path :read) 4)))
            "a fiber of chunks")
    (let [p (port/open path :read)
          [ok? err] (protect (framing:open p))]
      (port/close p)
      (assert (= :text-port (get err :reason)) "a text port is refused"))))
(def @served false)
(assert (= expected
           (lines-of (fn [n] (if served nil (begin (assign served true) (bytes text))))))
        "a function of n, answering nil at the end")
(assert (= expected
           (lines-of (fiber/new (fn []
                                  (yield (bytes "one\r\nt"))
                                  (yield "wo\nth")
                                  (yield (bytes "ree")))
                                |:yield|)))
        "a generator, yielding bytes or strings")
```

## Records are views

A record is bytes, and `(string rec)` is its text, itself a view. A record
inside the chunk is a `slice` of it: no byte moves. A record that straddles
two chunks is one `concat` of its parts. Finding the records inside a chunk
reads the source not at all, so a chunk of a hundred lines parks the fiber
once.

A view keeps its chunk's pages alive. A program that holds many records for a
long time bounds that with a smaller `:size`.
[framing-view.lisp](../../tests/impl/framing-view.lisp) pins the view.

## What each call answers

| Call | Before the end | At the end |
|------|----------------|------------|
| `line` | the line, without its `\n` and a `\r` before it | a partial last line whole, `\r` included; then `nil` |
| `until f delim` | the bytes before the delimiter, which is consumed | the rest; then `nil` |
| `exact f n` | exactly `n` bytes | `nil`, and the bytes it held go with it |
| `up-to f n` | up to `n` bytes of what the framer holds, after one fill when it holds nothing | `nil` |
| `rest` | everything left, as one value | empty bytes |
| `lines`, `records f delim` | a fiber yielding each record | the fiber ends, and closes nothing |

`delim` is bytes, a string, or one byte as an integer.

```lisp
(let [f (framing:open "a\r\nb\r")]
  (assert (= (bytes "a") (framing:line f)))
  (assert (= (bytes "b\r") (framing:line f)) "a partial last line keeps its CR")
  (assert (nil? (framing:line f))))

(let [f (framing:open "k=v;rest")]
  (assert (= (bytes "k=v") (framing:until f ";")))
  (assert (= (bytes "rest") (framing:until f ";")) "the rest, when the delimiter is never found")
  (assert (nil? (framing:until f ";"))))

(let [f (framing:open "abc")]
  (assert (nil? (framing:exact f 4)) "too few bytes answers nil")
  (assert (nil? (framing:exact f 1)) "and the three it held are gone"))

(let [f (framing:open "abcdef")]
  (assert (= (bytes "abcd") (framing:up-to f 4)) "up to four of the six")
  (assert (= (bytes "ef") (framing:rest f)) "everything left")
  (assert (= (bytes) (framing:rest f)) "empty bytes once nothing is left")
  (assert (nil? (framing:up-to f 1)) "and up-to answers nil"))

(let [f (framing:open "one\ntwo\n")]
  (assert (= 2 (length (stream/collect (framing:lines f))))))
(let [f (framing:open "k=v;rest")]
  (assert (= (list (bytes "k=v") (bytes "rest"))
             (stream/collect (framing:records f ";")))))
```

`line` answers what `port/read-line` answers on the same input, as bytes.
[framing.lisp](../../tests/lang/framing.lisp) pins the parity, and the lines
under every chunk size and every source kind.
[framing-records.lisp](../../tests/lang/framing-records.lisp) pins the other
calls, with every delimiter and count straddling a chunk edge.

## Invariants

1. One framer per source. Two framers over one source each hold a chunk the
   other cannot see, and so does the port itself: once a framer is over a
   port, read the port through the framer.
2. The framer never closes its source, and `lines` and `records` end without
   closing it, unlike `port/lines`.
3. `until` answers an empty record as empty bytes, and `nil` only at the end.

## A gather

A protocol often writes a message as several values: a status line, each
header, a blank line, a body. Joining them into one value before the write
copies the body, and writing each one as it comes parks the fiber once per
value. A gather does neither. `framing:write` queues the value itself, and
`framing:flush` hands the whole queue to one `port/write-many`, which sends
every part from the pages it lies in ([writing to a port](../io/write.md)).

```lisp
(with-temp-dir dir
  (let [path (path/join dir "message")
        body (bytes "hello")]
    (let [p (port/open-bytes path :write)
          g (framing:gather p)]
      (framing:write g "HTTP/1.1 200 OK\r\n")
      (framing:write g "content-length: 5\r\n\r\n")
      (framing:write g body)
      (assert (= 0 (length (file/read path))) "a write sends nothing")
      (framing:flush g)
      (port/close p))
    (assert (= "HTTP/1.1 200 OK\r\ncontent-length: 5\r\n\r\nhello" (file/read path))
            "the flush sends every write, in order")))
```

### The sink

`framing:gather` takes a port or a function of one argument, and never closes
it. A port receives each flush as one `port/write-many`. A function receives
the flush's parts as an array, and sends them as it can. A TLS connection
encrypts every byte into a record, which copies it anyway, so its sink joins
the parts with `framing:join` and makes one record of them.

```lisp
(def sent @[])
(def g (framing:gather (fn [parts] (push sent parts))))
(framing:write g "a")
(framing:write g (bytes "b"))
(framing:flush g)
(framing:flush g)
(assert (= [["a" (bytes "b")]] (freeze sent))
        "one call per flush that has something to send")
(assert (= (bytes "ab") (framing:join (get sent 0)))
        "join answers the parts as one bytes value")
```

### What a write queues

A write takes a string or bytes. An immutable value is queued as it is, and
nothing copies it. A mutable `@string` or `@bytes` is copied once when it is
written, so the flush sends the bytes the write was given, whatever the program
does to the store afterwards. An empty value queues nothing. Any other value
raises a `framing-error` whose `:reason` is `:not-bytes`, and queues nothing.

```lisp
(def got @[])
(def g (framing:gather (fn [parts] (push got parts))))
(def buf @b[1 2])
(framing:write g buf)
(put buf 0 9)
(framing:write g "")
(framing:flush g)
(assert (= [[(bytes 1 2)]] (freeze got)) "a mutable value is sent as it was written")
(let [[ok? err] (protect (framing:write g 42))]
  (assert (and (not ok?) (= :not-bytes (get err :reason)))
          "only strings and bytes are written"))
```

### Two fibers

A flush takes the queue before it sends it, and the gather starts a new queue.
The send parks the fiber, and another fiber can write and flush while it waits.
Its values go into the new queue, so they leave in a later send, and no flush
sends a value another flush already took.
[transport-writes.lisp](../../tests/lang/transport-writes.lisp) holds the HTTP
and HTTP/2 transports to it.

The order in which two fibers' sends reach the descriptor is the scheduler's,
not the gather's. Over a port, each send arrives whole, because a port sends
one write at a time ([writing to a port](../io/write.md)).
