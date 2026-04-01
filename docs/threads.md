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
open ports) will error at spawn time.

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
