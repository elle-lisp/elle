# Coroutines

Coroutines are cooperative generators. Create with `coro/new`, step
with `coro/resume`, suspend with `yield`.

## Basic usage

```lisp
(def co (coro/new (fn [] (yield 10) (yield 20) (yield 30))))

(coro/resume co)           # => 10
(coro/resume co)           # => 20
(coro/resume co)           # => 30
(coro/status co)           # => :paused (after final yield)
(coro/resume co)           # body completes
(coro/status co)           # => :dead
(coro/done? co)            # => true
```

## Lifecycle

```text
:new → :alive → :paused → :dead
         ↑         │
         └─────────┘ (resume)
```

- `:new` — created but never resumed
- `:alive` — currently running
- `:paused` — suspended at a `yield`
- `:dead` — body completed

`coro/value` returns the most recently yielded value without resuming.

## Generators

Coroutines naturally express infinite sequences:

```lisp
(defn make-fib []
  (coro/new (fn []
    (var a 0)
    (var b 1)
    (forever
      (yield a)
      (def next (+ a b))
      (assign a b)
      (assign b next)))))

(def fib (make-fib))
(coro/resume fib)          # => 0
(coro/resume fib)          # => 1
(coro/resume fib)          # => 1
(coro/resume fib)          # => 2
(coro/resume fib)          # => 3
(coro/resume fib)          # => 5
```

## Independent instances

Each `coro/new` call creates independent state:

```lisp
(def fib1 (make-fib))
(def fib2 (make-fib))
(coro/resume fib1)         # => 0
(coro/resume fib1)         # => 1
(coro/resume fib2)         # => 0 (independent)
```

## Streams

Coroutines are the basis of Elle's stream system. `stream/map`,
`stream/filter`, and `stream/take` compose lazily over coroutines.

```lisp
(defn naturals []
  (coro/new (fn []
    (var n 0)
    (forever (yield n) (assign n (+ n 1))))))

(stream/collect
  (stream/take 5
    (stream/map (fn [x] (* x x))
      (naturals))))
# => (0 1 4 9 16)
```

---

## See also

- [concurrency.md](concurrency.md) — async concurrency with ev/spawn
- [fibers](signals/fibers.md) — fiber architecture (coroutines are built on fibers)
- [control.md](control.md) — loops and iteration
