# Signals and JIT

## The Problem

The JIT can only compile silent functions naively because it can't handle
yields or errors — it would need to save and restore the native stack.

## The Solution

In the fiber model, a JIT-compiled function is just another frame on the
fiber's stack. When it calls a function that signals:

1. The callee returns `signal_bits` — the value is on the fiber
2. The JIT code checks `signal_bits == 0`
3. If zero: continue with the value from the fiber's stack
4. If non-zero: propagate the signal to the caller

The JIT doesn't need to capture continuations or switch stacks. It just
checks a return code and propagates. The JIT compiles silent, yielding,
and error-producing functions. Only polymorphic functions (where the
signal depends on arguments) and functions containing `Eval` are
rejected. The overhead for non-silent functions is one branch per call.

## Signal-guided optimization

The compiler's signal information guides JIT decisions:

- **No signals**: inline aggressively, no signal checks needed
- **Errors only**: signal checks needed, but no yield overhead
- **Yields**: full signal protocol, but still compiled — includes the
  propagation path
- **Known silent callback**: specialize inner loop to skip signal checks

## Silence bounds

Signal bounds enable further JIT optimizations:

1. **Loop specialization**: provably-silent callback → skip signal checks
2. **Inlining**: silent callbacks can be inlined more aggressively
3. **Elimination of polymorphism**: silence bounds simplify compilation

```text
# Without bounds: JIT cannot specialize
(defn map-any [f xs]
  (map f xs))

# With bounds: JIT can specialize for silent f
(defn map-silent [f xs]
  (silence f)
  (map f xs))
```

**Squelch and JIT:** Squelch is a runtime transform. The JIT uses the
underlying template signal (not the effective signal) for code generation.
Squelch enforcement happens at the call boundary in `call_inner`, not
inside JIT'd code.

---

## See also

- [Signal index](index.md)
- [Inference](inference.md) — compile-time signal verification
