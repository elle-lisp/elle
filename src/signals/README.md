# Signal System

The signal system tracks what operations a function can perform, enabling the compiler to make safe optimizations and the runtime to enforce constraints.

## The Three Signal Types

| Signal | Meaning | Examples |
|--------|---------|----------|
| `Inert` | No signals emitted, deterministic | Arithmetic, list operations, closures |
| `Yields` | May suspend execution via `yield` | Coroutines, generators, async operations |
| `Polymorphic` | Signal depends on arguments | Calls to functions with unknown signals |

## Signal Inference

Signals are inferred bottom-up during analysis:

1. **Literals and variables** are `Inert`
2. **Operators** (`+`, `-`, etc.) are `Inert`
3. **Control flow** (`if`, `begin`) combines signals of branches
4. **Function calls** inherit the callee's signal (or `Polymorphic` if unknown)
5. **Yield expressions** are `Yields`
6. **Lambda bodies** are analyzed, but the lambda itself is `Inert` (the signal is stored for later)

## Usage in the Compiler

The signal system enables:

- **Tail-call optimization**: Only in `Inert` contexts (no yield between call and return)
- **Dead code elimination**: `Inert` expressions with unused results can be removed
- **Scope allocation**: Scopes with `Inert` bodies can use region-based allocation
- **JIT compilation**: Only `Inert` functions can be JIT-compiled (no yield)

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/hir/`](../hir/) - where signals are inferred
- [`src/lir/lower/`](../lir/lower/) - where signals are used for optimization
