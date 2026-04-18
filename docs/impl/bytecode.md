# Bytecode

The bytecode instruction set is a `repr(u8)` enum. Operands follow
instructions inline in the bytecode stream.

## Instruction categories

### Stack operations
```text
LoadConst idx      push constant from pool
LoadLocal idx      push local variable
StoreLocal idx     pop into local variable
Dup                duplicate top of stack
Pop                discard top of stack
```

### Control flow
```text
Call argc          call function with argc arguments
TailCall argc      tail call (reuses frame)
Return             return top of stack
Jump offset        unconditional jump
JumpIfFalse offset branch if top is falsy
```

### Arithmetic
```text
Add                generic addition (any numeric type)
AddInt             specialized integer addition
Sub, Mul, Div      arithmetic ops
Mod, Rem           modulo and remainder
Neg                unary negation
```

### Comparison
```text
Eq                 structural equality
Lt, Gt, Le, Ge     ordering comparisons
```

### Type checks
```text
IsNil              test for nil
IsSymbol           test for symbol
IsArray            test for array
```

### Collections
```text
MakeArray n        construct array from n stack values
MakeStruct n       construct struct from n key-value pairs
```

### Fiber operations
```text
Yield              yield current fiber
Emit signal val    emit a signal
```

### Regions
```text
RegionEnter        begin scope allocation region
RegionExit         end scope allocation, free region objects
```

## Encoding

Instructions are encoded as a byte stream. The opcode byte is followed
by zero or more operand bytes (typically u16 or u32 indices). The
`LocationMap` maps bytecode offsets to source locations for error
reporting.

## Files

```text
src/compiler/bytecode.rs   Instruction enum and encoding
```

---

## See also

- [impl/vm.md](vm.md) — VM that executes bytecode
- [impl/lir.md](lir.md) — LIR that bytecode is emitted from
- [impl/jit.md](jit.md) — JIT alternative
- [impl/mlir.md](mlir.md) — MLIR/LLVM tier-2 backend
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute pipeline
