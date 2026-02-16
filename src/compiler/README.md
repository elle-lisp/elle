# Compiler

This module contains bytecode compilation infrastructure. It's a transitional
module: the legacy `Expr`-based pipeline coexists with the newer `HIR`→`LIR`
pipeline.

## For New Development

Use the new pipeline in `pipeline.rs`:

```rust
use elle::pipeline::compile_new;

let result = compile_new(source, &mut symbols)?;
vm.execute(&result.bytecode)?;
```

This goes through `Syntax` → `HIR` → `LIR` → `Bytecode` with proper binding
resolution and capture analysis.

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

## Effects

The `Effect` type tracks computational effects:

- `Pure` - no side effects, can be optimized
- `IO` - performs I/O or other side effects
- `Divergent` - may not terminate
- `Yields` - may yield (for coroutines)

Effects are inferred during HIR analysis and propagate upward. A function
calling an `IO` function is itself `IO`.

## JIT Compilation

Hot closures (called 10+ times) can be JIT-compiled via Cranelift:

```lisp
(define hot-fn (fn (x) (* x x)))
; After many calls...
(jit-compile hot-fn)  ; Returns JitClosure
```

The `JitCoordinator` tracks call counts. `JitClosure` contains a native code
pointer; the VM calls it directly instead of interpreting bytecode.

JIT is optional and requires:
- Cranelift support compiled in
- Function is `jit_compilable` (no unsupported operations)
- Bytecode fallback always available

## CPS (Continuation-Passing Style)

An alternative execution model where control flow is explicit:

```rust
pub enum Continuation {
    Return,
    Then { next: Box<Action>, cont: Box<Continuation> },
    // ...
}
```

Used by `coroutine-resume` to implement yield semantics. Not the primary
execution path but necessary for certain control flow patterns.

## Legacy Pipeline

The old pipeline converts `Value` → `Expr` → `Bytecode`:

```rust
let value = read_str(source, &mut symbols)?;
let expr = value_to_expr(&value, &mut symbols)?;
let bytecode = compile(&expr);
```

This still works but uses the older `VarRef` binding system. Prefer the new
pipeline for new code.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/hir/` - new HIR analysis
- `src/lir/` - new LIR lowering
- `src/vm/` - bytecode execution
