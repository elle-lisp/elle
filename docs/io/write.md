# Writing to a port

<!-- audited: 2026-11-11 -->

What `port/write` and `port/write-many` promise: every byte, in order, sent from where it lies.

[I/O](../io.md) holds ports and the scheduler that runs every call here.
`(doc name)` gives each function's arguments and answer.

## `port/write` writes every byte

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

The pinning tests are [port-shortwrite.lisp](../../tests/lang/port-shortwrite.lisp) and
[port-shortread-framing.lisp](../../tests/lang/port-shortread-framing.lisp) for the read direction.

## `port/write-many` sends its parts as one write

`port/write-many` takes an array or a list of strings and bytes. It sends
every byte of every part, in order, in one operation, and returns the sum of
the parts' lengths. It writes every byte as `port/write` does. So a message
built of a header and a body goes out in one write without first being joined
into one value, which `concat` would do by copying the body.

```lisp
(with-temp-dir dir
  (let [path (path/join dir "parts.txt")
        body (bytes "the body\n")]
    (let [w (port/open path :write)]
      (assert (= 16 (port/write-many w ["head:" "3\n" "" body]))
              "the parts' lengths, summed")
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

## What is sent from where it lies

An immutable string or bytes is sent from the region pages that hold it, by
address: no byte of it is copied on the way to the kernel. So are the parts
of an immutable array or list.

A value that can change while the write waits is copied once, when the call
is made, and the copy is what goes out. That covers a mutable `@string` or
`@bytes`, and parts held in an `@array`, whose parts another fiber can
replace. A write of such parts copies all of them, so the bytes sent are the
bytes the call was given.

[Where a stream operation's bytes live](../impl/io-bytes.md) holds the
argument, and names the tests that count the copies.
