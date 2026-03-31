# Concurrency

User code runs inside the async scheduler automatically. Use `ev/spawn`
and `ev/join` for concurrency — not OS threads.

## Spawn and join

```text
# Spawn a fiber, wait for its result
(def f (ev/spawn (fn [] (+ 1 2))))
(ev/join f)                # => 3

# Join a sequence — results in input order
(def [a b] (ev/join [(ev/spawn (fn [] :first))
                      (ev/spawn (fn [] :second))]))
```

## Parallel map

The most common pattern:

```text
(ev/map (fn [url] (http/get url)) urls)
# => [response1 response2 ...]

# Bounded parallelism (at most n in flight)
(ev/map-limited (fn [url] (http/get url)) urls 4)
```

## Error handling

`ev/join-protected` returns `[ok? value]` instead of raising errors:

```text
(let [[[ok? val] (ev/join-protected (ev/spawn (fn [] (flaky-call))))]]
  (if ok? val (fallback)))
```

## Select, race, timeout

```text
# First to complete wins; abort the rest
(ev/race [(ev/spawn (fn [] (query-replica-1)))
          (ev/spawn (fn [] (query-replica-2)))])

# Deadline on a computation
(ev/timeout 5 (fn [] (slow-operation)))

# Wait for first of N — returns [done remaining]
(ev/select fibers)
```

## Scoped concurrency

All children must finish before scope exits. If one child fails, the
others are aborted.

```text
(ev/scope (fn [spawn]
  (let [[users    (spawn (fn [] (fetch "/users")))]
        [settings (spawn (fn [] (fetch "/settings")))]]
    {:users (ev/join users) :settings (ev/join settings)})))
```

## Primitives reference

```text
(ev/spawn thunk)            # create fiber, returns handle
(ev/join fiber-or-seq)      # wait for result(s), propagate errors
(ev/join-protected target)  # wait without raising — [ok? value]
(ev/abort fiber)            # graceful cancel (defer blocks run)
(ev/select fibers)          # wait for first → [done remaining]
(ev/race fibers)            # first wins, abort rest, return value
(ev/timeout secs thunk)     # deadline — value or {:error :timeout}
(ev/scope (fn [spawn] ...)) # nursery — children can't outlive scope
(ev/map f items)            # parallel map, results in order
(ev/map-limited f items n)  # bounded parallel map
(ev/as-completed fibers)    # lazy iterator → [next-fn pool]
(ev/sleep seconds)          # yield for N seconds
```

## TCP

```text
(tcp/listen addr port)      # bind and listen, returns listener
(tcp/accept listener)       # yield until connection, returns port
(tcp/connect host port)     # yield until connected, returns port
```

---

## See also

- [fibers.md](fibers.md) — fiber architecture
- [io.md](io.md) — port I/O
- [threads.md](threads.md) — OS threads
- [signals.md](signals.md) — signal system
