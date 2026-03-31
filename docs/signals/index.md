# Signals

Elle's unified signal system. Signals are the mechanism for all non-local
control flow: errors, yields, I/O, fuel exhaustion.

| File | Content |
|------|---------|
| [design](design.md) | Motivation, prior art, terminology, core insight |
| [protocol](protocol.md) | Signal protocol, registry, user signals |
| [inference](inference.md) | Compile-time verification, restrictions |
| [jit](jit.md) | JIT integration |
| [recovery](recovery.md) | Non-unwinding recovery, error signalling |
| [questions](questions.md) | Open and resolved design questions |
| [fibers](fibers.md) | Fiber architecture (shared topic) |
| [primitives](primitives.md) | Fiber primitives, cancel vs abort |
