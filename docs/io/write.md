# Writing to a port

<!-- audited: 2026-11-12 -->

What `port/write` and `port/write-many` promise: every byte, in order, sent from where it lies.

[I/O](../io.md) holds ports and the scheduler that runs every call here.
`(doc name)` gives each function's arguments and answer.

## `port/write` writes every byte

`port/write` returns the number of bytes in the data you gave it, so `"é"`
counts two. The caller never loops on the return value.

One `write(2)` transfers only what fits in the descriptor's send buffer at that
moment. On a socket that is often far less than the payload. A 4 KiB send
buffer accepts about 21 KB of a 200 KB write, and a default one a few megabytes
of an 8 MB write. The backend writes the rest before the call returns
([writes](../impl/io/write.md)). `port/read` is the deliberate opposite: it
returns up to n bytes, as POSIX does, and `port/read-exact` is its
all-or-nothing sibling.

If the descriptor fails part of the way through, `port/write` raises the error
rather than returning a short count. An unknown prefix of the payload reached
the peer in that case, the same guarantee `write(2)`-loop helpers give
elsewhere.

The pinning tests are [port-shortwrite.lisp](../../tests/lang/port-shortwrite.lisp)
and, for the read direction,
[port-shortread-framing.lisp](../../tests/lang/port-shortread-framing.lisp).

## `port/write-many` sends its parts as one write

`port/write-many` takes an array or a list of strings and bytes. It sends
every byte of every part, in order, in one operation, and returns the number
of bytes in all its parts. It writes every byte as `port/write` does. So a
message built of a header and a body goes out in one write without first being
joined into one value, which `concat` would do by copying the body.

```lisp
(with-temp-dir dir
  (let [path (path/join dir "parts.txt")
        body (bytes "the body\n")]
    (let [w (port/open path :write)]
      (assert (= 16 (port/write-many w ["head:" "3\n" "" body]))
              "the bytes of every part, summed")
      (assert (= 3 (port/write-many w (list "a" (bytes "b") "c")))
              "a list of parts works too")
      (assert (= 0 (port/write-many w [])) "no parts writes nothing")
      (port/close w))
    (assert (= "head:3\nthe body\nabc" (file/read path))
            "the parts arrive in order")))
```

A part that is not a string or bytes raises a `type-error`, and so does a
`parts` that is not an array or a list. Either one raises before a byte is
written.

```lisp
(with-temp-dir dir
  (let [w (port/open (path/join dir "refused.txt") :write)]
    (let [[ok? err] (protect (port/write-many w ["a" 42]))]
      (assert (and (not ok?) (= :type-error (get err :error)))
              "a part must be a string or bytes"))
    (let [[ok? err] (protect (port/write-many w "a"))]
      (assert (and (not ok?) (= :type-error (get err :error)))
              "the parts come in an array or a list"))
    (port/close w)))
```

[port-write-many.lisp](../../tests/lang/port-write-many.lisp) pins both
calls, across short writes and on each backend.

## Writes to one port go out one at a time

Writes to one port go out one at a time, in the order the calls reach the
backend, whether `port/write` or `port/write-many` makes them. A write that
finds another in flight on its port waits for it, so two fibers that write to
one socket never interleave their bytes. A write cancelled while it waits sends
nothing. [I/O deadlines](timeout.md) says how its bounds count the wait.

```lisp
(ev/run (fn []
          (let* [listener (tcp/listen "127.0.0.1" 0)
                 address (port/path listener)
                 conn (tcp/connect "127.0.0.1"
                                   (parse-int (slice address (+ 1 (string/find address ":"))))
                                   :sndbuf 4096)
                 peer (tcp/accept listener)
                 a (bytes (string/repeat "a" 200000))
                 b (bytes (string/repeat "b" 100000))
                 c (bytes (string/repeat "c" 100000))
                 writers [(ev/spawn (fn [] (port/write conn a)))
                          (ev/spawn (fn [] (port/write-many conn [b c])))]
                 got (port/read-exact peer 400000)]
            (ev/join writers)
            (assert (or (= got (concat a b c)) (= got (concat b c a)))
                    "each write arrives whole, one after the other")
            (port/close peer)
            (port/close conn)
            (port/close listener))))
```

## What is sent from where it lies

An immutable string or bytes is sent from the region pages that hold it, by
address: no byte of it is copied on the way to the kernel. So are the parts
of an immutable array or list.

A value that can change while the write waits is copied once, when the call
is made, and the copy is what goes out. The wait includes a wait in line
behind another write on the port. That covers a mutable `@string` or
`@bytes`, and parts held in an `@array`, whose parts another fiber can
replace. A write of such parts copies all of them, so the bytes sent are the
bytes the call was given.
[port-write-many-changes.lisp](../../tests/lang/port-write-many-changes.lisp)
changes such parts while the write waits.

[Where a stream operation's bytes live](../impl/io/bytes.md) holds the
argument, and names the tests that count the copies.
