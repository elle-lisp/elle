# Threads

OS threads for CPU-bound work. For I/O-bound concurrency, prefer
`ev/spawn` / `ev/join` (see [concurrency.md](concurrency.md)).

## spawn and join

```text
(def handle (sys/spawn (fn [] (+ 1 2))))
(sys/join handle)          # => 3
(sys/thread-id)            # current OS thread ID
```

## Channels

Crossbeam-based channels for inter-fiber and inter-thread messaging.

```text
(def [tx rx] (chan))           # unbounded channel
(def [tx rx] (chan 10))        # bounded (capacity 10)

(chan/send tx 42)              # => [:ok], [:full], or [:disconnected]
(chan/recv rx)                 # => [:ok msg], [:empty], or [:disconnected]

(chan/clone tx)                # clone sender (multiple producers)
(chan/close tx)                # close sender half
(chan/close-recv rx)           # close receiver half

# Multiplex: block until one receiver has data
(chan/select @[r1 r2])         # => [index msg] or [:disconnected]
(chan/select @[r1 r2] 1000)   # with timeout (ms)
```

---

## See also

- [concurrency.md](concurrency.md) — async concurrency with ev/spawn
- [fibers.md](fibers.md) — fiber architecture
