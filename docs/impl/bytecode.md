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
Add, Sub, Mul, Div              generic arithmetic (any numeric type)
Rem                             remainder
AddInt, SubInt, MulInt, DivInt  integer-specialized arithmetic
```
The integer-only forms are implemented but unemitted. The interpreter runs
them (`src/vm/arithmetic.rs`), and nothing produces them: the emitter maps
every LIR `BinOp` to the polymorphic bytecode
(`src/lir/emit/instr/ops.rs`), so a program never reaches an `AddInt`.
Wiring them up is tracked as issue #957, which carries the compiler's
operand-type proofs across the HIR→LIR boundary and spends them here.

There is no negation instruction, and no modulo instruction. The emitter
lowers unary minus to a `Mul` by the constant `-1`.

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
Pair rid            construct a pair in region rid from two stack values
EmptyList           push the empty list
First, Rest         pair accessors
MakeArrayMut rid n  construct an @array in region rid from n stack values
ArrayMutRef         read an @array element; the index comes off the stack
ArrayMutSet         write an @array element; index and value come off the stack
ArrayMutLen         @array length
StructRest count    copy a struct minus count excluded keys
```
`MakeArrayMut` is the only instruction that builds an array from stack values,
and no instruction builds a struct. An immutable array or struct reaches the
stack from `MaterializeConst`, which materializes a literal into a fresh region,
or from `IntrFreeze`, which copies a mutable collection.

### Fiber operations
```text
Emit bits          emit a signal; the value comes off the stack
```
`Emit` is the only instruction that suspends a fiber. The operand selects the
signal: `(emit :yield v)` emits `SIG_YIELD`, and `(emit :io v)` emits `SIG_IO`.
See [impl/vm.md](vm.md) — *Fiber integration*.

### Self-reference
```text
LoadSelf           push the currently-executing closure (no operand)
```
`LoadSelf` is the value path for a closure that references itself — passed to a
higher-order call, returned, or stored, then invoked. It reads the runtime's
per-activation executing-closure register, so a value-position self-reference
resolves to the closure itself with no capture-slot operand. See
[impl/lir.md](lir.md) — *Self-reference: `LoadSelf`*.

### Regions
```text
IncrefRegion rid   increment region rid's reference count
DecrefRegion rid   decrement region rid; free pages when RC hits 0
```
`DecrefRegion` is the only region-demise bytecode; there is no
separate `FreeRegion`. See `docs/regions.md` for the full model.

## Encoding

Instructions are encoded as a byte stream. The opcode byte is followed
by zero or more operand bytes (typically u16 or u32 indices). The
`LocationMap` maps bytecode offsets to source locations for error
reporting.

### Signal-bits operands

`SignalBits` is a 64-bit mask, and every bit of it is meaningful in
bytecode: built-in signals sit at bits 0–17, the runtime reserves bits
18–31, and `(signal :keyword)` allocates user signals from bit 32 upward
(`docs/signals/protocol.md`). An operand that holds fewer than 64 bits
therefore cannot name a user signal at all.

Two instructions carry such an operand — `Emit` and `CheckSignalBound` —
and both encode it the same way: eight bytes, big-endian, written by
`Bytecode::emit_signal_bits` and read by `VM::read_signal_bits`. Use that
pair rather than an open-coded byte sequence; a hand-written operand is
how a mask silently loses its high half.

## Files

```text
src/compiler/bytecode.rs              Bytecode struct, encoding, disassembly entry points
src/compiler/bytecode/instruction.rs  Instruction enum and opcode decoding
src/compiler/bytecode/disasm.rs       the disassembler
```

---

## See also

- [impl/vm.md](vm.md) — VM that executes bytecode
- [impl/lir.md](lir.md) — LIR that bytecode is emitted from
- [impl/jit.md](jit.md) — JIT alternative
- [impl/mlir.md](mlir.md) — MLIR/LLVM tier-2 backend
- [impl/wasm.md](wasm.md) — WebAssembly backend
- [impl/gpu.md](gpu.md) — GPU compute pipeline
