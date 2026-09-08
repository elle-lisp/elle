# Signals

<!-- audited: 2026-09-07 -->

Elle's unified signal and capability system. Two directions:

- **Signals flow up** — from callee to caller. Inferred at compile time,
  emitted at runtime. Every form of non-local control flow (errors, yields,
  I/O, fuel exhaustion) is a signal.
- **Capabilities flow down** — from parent fiber to child. A parent
  withholds capabilities via `:deny`; denied operations become signals the
  parent can catch and mediate.

| File | Content |
|------|---------|
| [emit](emit.md) | `emit` special form, yield/error macros, signal emission |
| [capabilities](capabilities.md) | Capability enforcement, `:deny`, `fiber/caps` |
| [authority](authority.md) | What holds authority, and where a fiber's right to spend it is asked |
| [design](design.md) | Motivation, prior art, terminology, core insight |
| [protocol](protocol.md) | Signal protocol, registry, user signals |
| [inference](inference.md) | Compile-time verification, restrictions |
| [jit](jit.md) | JIT integration |
| [recovery](recovery.md) | Non-unwinding recovery, error signalling |
| [questions](questions.md) | Open and resolved design questions |
| [fibers](fibers.md) | Fiber architecture (shared topic) |
| [primitives](primitives.md) | Fiber primitives, cancel vs abort |
