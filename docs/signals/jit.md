# Signals and JIT

<!-- audited: 2026-10-06 -->

A function's signal decides nothing about whether the JIT compiles it. It
decides the checks around each call and how a signal leaves compiled code.

## What the JIT refuses

The JIT compiles silent, failing, yielding and polymorphic functions alike.
It refuses two shapes, whatever their signal:

- a function that contains `MakeClosure`, that is, one that creates a closure;
- a function with a `&keys` or `&named` collector.

A refused function runs in the interpreter, and the VM never submits it again.
`(jit/rejections)` lists each refusal with its reason, for example
`"JIT: unsupported instruction: MakeClosure"`.
[src/jit/AGENTS.md](../../src/jit/AGENTS.md) lists the supported instructions.

## Signals at a call

A compiled function in a fiber is one more frame on that fiber. Every call it
makes leaves through a runtime dispatch helper; compiled functions never call
each other directly. After the call returns, the compiled code checks whether
the callee signalled. On a halt it returns to its caller at once. On any other
signal, an error included, it builds its own suspended frame and returns to the
interpreter, because an error parks the frames it passes as a yield does.

The emitter records resume metadata for the calls of a function that is not
silent, and for each instruction in it that may raise. An error-only function
counts as not silent here. A silent function records no site: the only
signals that can reach it are a halt and the terminal error a broken proof
raises, and it returns to its caller on either.

## Yielding from compiled code

A `yield` or an `error` inside a compiled function leaves by side-exit. The code spills its
live registers, a runtime helper builds a suspended frame at the matching
bytecode offset, and the function returns a sentinel. When the fiber resumes,
the interpreter continues from that frame. The compiled code never saves or
switches a native stack.

## Bounds and boundaries

A `(bound p spec)` bound is checked at function entry in compiled code as in
the interpreter; see [inference.md](inference.md). A closure's squelch mask is
checked on the JIT's call, tail-call and sentinel paths, and every one of those
paths asks the same predicate, `signals::squelched_bits`, that the interpreter
asks.

The same paths check the boundary of a silent function. A silent function is
a proof that nothing stops the fiber, so a signal that leaves one raises the
terminal `:broken-proof` error on every tier. A halt, and a signal that
already carries `:terminal`, pass it as themselves
([inference.md](inference.md)). Compiled code charges no fuel, and the
interpreter charges none inside a silent function, so no tier pauses one.

## See also

- [Signal index](index.md)
- [Inference](inference.md) — the signal each function carries
- [JIT implementation](../impl/jit.md) — the caches, the worker and rejection
  tracking
