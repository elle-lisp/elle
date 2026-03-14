# JIT Compilation

The JIT subsystem compiles Elle bytecode to native machine code using Cranelift, enabling significant performance improvements for compute-intensive code.

## How JIT Works

1. **Eligibility Check**: Only `Silent` functions (no `yield`) can be JIT-compiled
2. **Compilation**: Bytecode is translated to Cranelift IR and compiled to native code
3. **Caching**: Compiled code is cached on the `Closure` object
4. **Execution**: The VM dispatches to native code instead of interpreting bytecode

## Supported Operations

JIT compilation supports:

- **Arithmetic**: `+`, `-`, `*`, `/`, `mod`, `rem`, `abs`, `min`, `max`
- **Comparison**: `<`, `>`, `<=`, `>=`, `=`
- **Logic**: `and`, `or`, `not`
- **Type checks**: `integer?`, `float?`, `number?`, etc.
- **List operations**: `cons`, `car`, `cdr`, `length`
- **Control flow**: `if`, `begin`, `cond`

## Limitations

JIT compilation is disabled for:

- Functions with `yield` (suspendable functions)
- Functions that call other functions (interprocedural analysis needed)
- Functions with complex control flow (future optimization)

## Performance

JIT compilation typically provides 5-10x speedup for numeric code. Benchmarks are in [`benches/`](../../benches/).

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`src/compiler/bytecode.rs`](../compiler/bytecode.rs) - bytecode instruction definitions
- [`benches/`](../../benches/) - performance benchmarks
