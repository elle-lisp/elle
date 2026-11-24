# Signals and JIT

<!-- audited: 2026-11-23 -->

A function's signal decides nothing about whether the JIT compiles it. It
decides the checks around each call and how a signal leaves compiled code.

## What the JIT refuses

The JIT compiles silent, failing, yielding and polymorphic functions alike.
Whatever the signal, it refuses a function with a `&keys` or `&named`
collector, and a function that calls `eval`.

A refused function runs in the interpreter, and the VM never submits it again.
`(jit/rejections)` lists each refusal with its reason, for example
`"JIT: unsupported instruction: Eval"`.
[src/jit/AGENTS.md](../../src/jit/AGENTS.md) lists the supported instructions.

## Signals at a call

A compiled function in a fiber is one more frame on that fiber. Every call it
makes leaves through a runtime dispatch helper, except a call to itself with
immediate arguments ([jit-selfcall.md](../impl/jit-selfcall.md)). After the
call returns, the compiled code checks whether the callee signalled. On a halt
it returns to its caller at once. On any other signal, an error included, it
builds its own suspended frame and returns to the interpreter, because an
error parks the frames it passes as a yield does.

The emitter records resume metadata for the calls of a function that is not
silent, and for each instruction in it that may raise. An error-only function
counts as not silent here. A silent function records no site: the only
signals that can reach it are a halt and a terminal error, which a broken
proof or a kill raises, and it returns to its caller on either.

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

A direct self-call checks no boundary, and the answer is the same
([jit-selfcall.md](../impl/jit-selfcall.md)). Every other call into compiled
code builds the callee's boundary only when a signal or a tail call comes
back. Before the call it copies the closure's squelch mask and its code
object, and after the call it decides from the code object whether the
function is silent. Nothing reads the closure after the call returns, because
no rule keeps a callee's closure alive through its own call. The code object
stays readable: the JIT cache pins the region it lives in
([jit.md](../impl/jit.md)).

The same paths check the boundary of a silent function. A silent function is
a proof that nothing stops the fiber, so a signal that leaves one raises the
terminal `:broken-proof` error on every tier. A halt, and a signal that
already carries `:terminal`, pass it as themselves
([inference.md](inference.md)). No tier pauses a silent function: its
charges spend fuel, and none of them pauses the fiber. A kill ends it as it
ends any function ([runtime](../runtime.md)).

## Fuel

Compiled code charges fuel at the instructions where the interpreter charges
it. In a function that is not silent, a charge that finds the budget spent
parks the frame at the instruction that charged, and the interpreter runs that
instruction again on resume. In a silent function a charge takes a unit while
one is left and never parks, so a silent function records no charge site, as
it records no call site ([jit-fuel.md](../impl/jit-fuel.md)). Every charge on either
tier reads the kill word first, and a killed worker leaves compiled code with
the terminal error, through the check after the call in each compiled caller
([runtime](../runtime.md)).

## See also

- [Signal index](index.md)
- [Inference](inference.md) — the signal each function carries
- [JIT implementation](../impl/jit.md) — the caches, the worker and rejection
  tracking
