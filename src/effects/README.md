# Effects System

The effect system tracks what operations a function can perform, enabling the compiler to make safe optimizations and the runtime to enforce constraints.

## The Three Effects

| Effect | Meaning | Examples |
|--------|---------|----------|
| `Pure` | No side effects, deterministic | Arithmetic, list operations, closures |
| `Yields` | May suspend execution via `yield` | Coroutines, generators, async operations |
| `Polymorphic` | Effect depends on arguments | Calls to functions with unknown effects |

## Effect Inference

Effects are inferred bottom-up during analysis:

1. **Literals and variables** are `Pure`
2. **Operators** (`+`, `-`, etc.) are `Pure`
3. **Control flow** (`if`, `begin`) combines effects of branches
4. **Function calls** inherit the callee's effect (or `Polymorphic` if unknown)
5. **Yield expressions** are `Yields`
6. **Lambda bodies** are analyzed, but the lambda itself is `Pure` (the effect is stored for later)

## Usage in the Compiler

The effect system enables:

- **Tail-call optimization**: Only in `Pure` contexts (no yield between call and return)
- **Dead code elimination**: `Pure` expressions with unused results can be removed
- **Scope allocation**: Scopes with `Pure` bodies can use region-based allocation
- **JIT compilation**: Only `Pure` functions can be JIT-compiled (no yield)

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/hir/`](../hir/) - where effects are inferred
- [`src/lir/lower/`](../lir/lower/) - where effects are used for optimization
