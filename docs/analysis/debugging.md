# Debugging and introspection

<!-- audited: 2026-10-06 -->

The primitives a program uses to print its values, inspect a closure, and time its own work.

## Printing a value on its way through

| Primitive | Does |
|-----------|------|
| `debug/print` | writes `[DEBUG] value` to stderr and answers the value |
| `debug/trace` | writes `[TRACE] label: value` to stderr and answers the value; the label is a string or a symbol |
| `debug/memory` | answers `(rss-bytes virtual-bytes)` for the process |

Each print answers its argument, so it can wrap an expression without
changing what the program computes.

```lisp
(assert (= (debug/print (+ 1 2)) 3) "debug/print answers its argument")
(assert (= (debug/trace "sum" (+ 1 2)) 3) "debug/trace answers its value")
```

## Inspecting a closure

Each primitive below takes any value. A value that is not a closure answers
`false`, or `nil` where the answer is a number; `call-count` answers `0`. A
native primitive such as `clock/monotonic` is not a closure.

| Primitive | Answers |
|-----------|---------|
| `closure?` | `true` for a closure |
| `fn/errors?` | `true` when the closure's inferred signal carries `:error` |
| `silent?` | `true` when the inferred signal has no bits and is not polymorphic; `:error` alone makes it `false` |
| `jit?` | `true` when the JIT has compiled the closure; always `false` in a build without the `jit` feature |
| `fn/mutates-params?` | `true` when the body assigns one of its own parameters |
| `fn/arity` | an int for an exact arity, `(min . max)` with `&opt`, `(min . nil)` with a rest parameter |
| `fn/captures` | the number of values in the closure's environment |
| `fn/bytecode-size` | the length of the closure's bytecode in bytes |
| `call-count` | the calls the VM has counted for the closure |
| `fiber?` | `true` for a fiber |
| `global?` | always `false`: a program has no runtime globals |

`mutates-params?`, `arity`, `captures` and `bytecode-size` are aliases of the
`fn/` names. `(doc name)` gives each one's docstring.

The VM counts a closure's calls only while the JIT is on, or under `--wasm=N`.
An `mlir` or `wasm` build starts with the JIT off ([config](../config.md)), so
there `call-count` answers `0` for a closure the program has called.

```lisp
(defn add [a b]
  (+ a b))
(assert (closure? add) "a defn is a closure")
(assert (not (closure? clock/monotonic)) "a native primitive is not")
(assert (fn/errors? add) "+ may signal :error, so add may too")
(assert (not (silent? add)) "and an :error signal is enough to not be silent")
(assert (silent? (fn [x] x)) "the identity has no signal at all")
(defn reset [@x]
  (assign x 0)
  x)
(assert (fn/mutates-params? reset) "reset assigns its own parameter")
(assert (= (fn/arity add) 2))
(assert (= (fn/arity (fn [a &opt b] a)) (pair 1 2)))
(assert (= (fn/arity (fn [a & more] a)) (pair 1 nil)))
(assert (nil? (fn/arity clock/monotonic)) "a native primitive has no answer")
(def sum-of-two
  (let [a 1
        b 2]
    (fn [] (+ a b))))
(assert (= (fn/captures sum-of-two) 2) "sum-of-two captures a and b")
(assert (> (fn/bytecode-size add) 0))
(add 1 2)
(add 3 4)
(assert (= (call-count add) (if (vm/config :jit) 2 0))
        "the VM counted both calls, if the JIT is on")
(assert (= (call-count 42) 0) "a value that is not a closure counts no calls")
```

`fn/mutates-params?` reads the parameters the body assigns, which the compiler
wraps in capture cells. It says nothing about an outer binding the closure
captures and assigns.

The compiler infers the signal that `fn/errors?` and `silent?` read.
[Signal inference](../signals/inference.md) says how.

## Timing

A time value is a float in seconds. A float composes with arithmetic: `(- end
start)` is a duration, and `(< a b)` orders two readings. An f64 holds a
duration to the nanosecond for about 52 days, and a wall-clock reading to
better than a microsecond.

| Primitive | Answers |
|-----------|---------|
| `clock/monotonic` | seconds since this process first read the clock; it never goes back |
| `clock/realtime` | seconds since the Unix epoch, by the wall clock, which can jump |
| `clock/cpu` | the CPU time the calling thread has used, in seconds |
| `time/elapsed` | `(result seconds)` for a thunk it calls |
| `time/stopwatch` | a fiber whose every resume yields the seconds since the stopwatch was made |
| `time/sleep` | `nil`, after blocking the thread for a number of seconds |

A `:deadline` is a `clock/monotonic` reading ([I/O deadlines](../io/timeout.md)).

Use `clock/cpu` to tell computation from waiting. On Linux it is a system call
rather than a vDSO read, so each call costs several hundred nanoseconds more
than `clock/monotonic`.

`time/sleep` stops every fiber on the thread until it returns. `ev/sleep`
yields to the scheduler instead ([concurrency](../concurrency.md)).

```lisp
(let [(result seconds) (time/elapsed (fn [] (+ 1 2)))]
  (assert (= result 3) "time/elapsed answers the thunk's result")
  (assert (>= seconds 0) "and the seconds it took"))

(def stopwatch (time/stopwatch))
(def first-reading (fiber/resume stopwatch))
(time/sleep 0.01)
(def second-reading (fiber/resume stopwatch))
(assert (>= second-reading 0.01) "a stopwatch reading is cumulative")
(assert (> second-reading first-reading))
```

For timing a hot path, subtract two `clock/monotonic` readings. A stopwatch
resumes a fiber on every reading.
