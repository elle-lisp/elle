# Squelch and attune

<!-- audited: 2026-11-16 -->

The boundary `squelch` and `attune` put around a closure, and the narrower signal a binding of one takes at compile time.

[Inference](inference.md) owns the signal the compiler infers for a function.
The examples below read a signal with this helper:

```lisp
(defn signal-of [src name]
  (compile/signal (compile/analyze src) name))
```

## The boundary

`squelch` and `attune` are primitives, not declarations. They are the
boundary a caller puts around a closure it did not write. Each returns a new
closure that shares the original's bytecode and environment. When the new
closure returns a signal its mask covers, the boundary turns that signal into
a `:signal-violation` error.

Both take the spec first. `spec` is a keyword, a set, an array or list of
keywords, or an integer.

- `(squelch spec f)` blocks the signals in `spec` and lets the rest through.
- `(attune spec f)` lets through only the signals in `spec` and blocks the
  rest. `(squelch spec f)` is therefore `(attune (complement spec) f)`.

```lisp
(defn producer [] (yield 1))

(def [ok? err] (protect ((squelch :yield producer))))
(assert (= :signal-violation (get err :error)))
(assert (= "squelch: signal {:yield} caught at boundary" (get err :message)))

(def [ok? err] (protect ((attune |:yield :error| (fn [] (println "hi"))))))
(assert (= "squelch: signal {:io} caught at boundary" (get err :message)))

(def [ok? err] (protect (squelch producer :yield)))
(assert (= "squelch: second argument must be a closure, got keyword"
           (get err :message)))
```

Layers compose: a squelch of a squelched closure blocks both masks. The check
also fires when the squelched closure is called in tail position.

```lisp
(def quiet (squelch :io (squelch :yield (fn [] (println "hi")))))
(assert (= :signal-violation (get (get (protect (quiet)) 1) :error)))

(def blocked (squelch :yield producer))
(defn tail-call [] (blocked))
(assert (= :signal-violation (get (get (protect (tail-call)) 1) :error)))
```

Three classes of signal cross a `squelch` or `attune` boundary untouched.
`:error` and `:halt` pass whole, so squelching `:error` does nothing. The VM's
`:switch` trampoline passes. A signal that carries `:fuel` is a pause the VM
injects under code that is not silent, and it passes whole too. A fuel stop
and a dynamic breakpoint's `|:debug :fuel|` are such pauses.
[signals/mod.rs](../../src/signals/mod.rs) holds the one predicate,
`squelched_bits`, that every tier asks.

```lisp
(def [ok? err] (protect ((squelch :error (fn [] (error {:error :mine}))))))
(assert (= :mine (get err :error)) "squelch never blocks :error")

(def paused (fiber/new (fn [] ((squelch :debug (fn [] (emit |:debug :fuel|) 5))))
                       |:debug :fuel :error|))
(fiber/resume paused)
(assert (= :paused (fiber/status paused)))
(assert (nil? (fiber/value paused)) "the pause passes a squelch of :debug as itself")
```

## Narrowing at compile time

When a `def` or `let` binds `(squelch spec f)` or `(attune spec f)` with a
literal spec, the analyzer computes the new closure's signal. It removes the
bits the boundary blocks, and adds `:error` when it removed any. The signals
that cross a boundary untouched are never blocked, so `:error` and `:halt` stay.
A caller of the binding then sees the narrower signal. A literal spec is a
keyword or a set literal. A set literal is a call to `set`, so where a binding
shadows `set` the spec is computed, and the binding is not narrowed.

```lisp
(def sig (signal-of "(defn p [] (yield 1))
(def safe (squelch :yield p))
(defn use [] (safe))" :use))
(assert (= |:error| (get sig :bits)))
```

A closure that may raise every bit narrows too. Here `p` calls a binding that
an `assign` reaches, so `p` may raise anything, and `attune` leaves it the bits
its spec names, with `:error` and `:halt`.

```lisp
(def sig (signal-of "(def @h nil) (assign h nil) (defn p [] (h))
(def safe (attune :yield p))
(defn use [] (safe))" :use))
(assert (= |:yield :error :halt| (get sig :bits)))

(def sig (signal-of "(defn p [] (sys/exit 0))
(def safe (squelch :halt p))
(defn use [] (safe))" :use))
(assert (contains? (get sig :bits) :halt) "a halt crosses the boundary")
```

## See also

- [Signal index](index.md)
- [Signals and JIT](jit.md)
