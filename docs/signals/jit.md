# Signals and JIT

<!-- audited: 2026-09-22 -->

A function's signal decides nothing about whether the JIT compiles it. It
decides the checks around each call and how a yield leaves compiled code.

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
each other directly. After the call returns:

1. The compiled code checks for a pending error, and on one it returns to its
   caller through the error exit.
2. In a function that is not silent, it also checks whether the callee yielded.
   On a yield it builds its own suspended frame and returns to the interpreter.

A silent function skips the second check. The emitter records resume metadata
only for the calls of a function that is not silent. An error-only function
counts as not silent here.

## Yielding from compiled code

A `yield` inside a compiled function leaves by side-exit. The code spills its
live registers, a runtime helper builds a suspended frame at the matching
bytecode offset, and the function returns a sentinel. When the fiber resumes,
the interpreter continues from that frame. The compiled code never saves or
switches a native stack.

## Bounds and squelch

A `(silence p)` bound is checked at function entry in compiled code as in the
interpreter; see [inference.md](inference.md). A squelched closure is checked
on the JIT's call, tail-call and sentinel paths. Every one of them asks the
same predicate, `signals::squelched_bits`, that the interpreter asks.

## See also

- [Signal index](index.md)
- [Inference](inference.md) — the signal each function carries
- [JIT implementation](../impl/jit.md) — the caches, the worker and rejection
  tracking
