# Compiler

This module contains bytecode instruction definitions and debug formatting.

## Bytecode

The `Instruction` enum defines all VM operations:

```rust
pub enum Instruction {
    LoadConst = 0,
    LoadLocal = 1,
    StoreLocal = 2,
    // ...
}
```

`Bytecode` bundles instructions with constants:

```rust
pub struct Bytecode {
    pub instructions: Vec<u8>,
    pub constants: Vec<Value>,
}
```

## Compilation Pipeline

Compilation uses the HIR → LIR → Bytecode pipeline:

```rust
use elle::pipeline::compile_new;

let result = compile_new(source, &mut symbols)?;
vm.execute(&result.bytecode)?;
```

This goes through `Syntax` → `HIR` → `LIR` → `Bytecode` with proper binding
resolution, capture analysis, and source location tracking.

## Effects

The `Effect` type (in `src/effects/`) tracks computational effects:

- `Pure` - no side effects, can be optimized
- `Yields` - may yield (for coroutines)
- `Polymorphic` - effect depends on arguments

Effects are inferred during HIR analysis and propagate upward. A function
calling a `Yields` function is itself `Yields`.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/hir/` - HIR analysis
- `src/lir/` - LIR lowering and emission
- `src/vm/` - bytecode execution
