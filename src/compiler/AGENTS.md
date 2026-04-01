# compiler

Bytecode instruction definitions and debug formatting.

## Responsibility

- Define the `Instruction` enum (bytecode opcodes)
- Define the `Bytecode` struct (instructions + constants)
- Provide debug formatting for bytecode disassembly

## Submodules

| Module | Purpose |
|--------|---------|
| `bytecode.rs` | `Instruction` enum, `Bytecode` struct |
| `bytecode_debug.rs` | Debug formatting for bytecode disassembly |

## Dependents

- `pipeline.rs` - uses `Bytecode`
- `lir/emit.rs` - emits `Instruction` bytes
- `vm/` - executes bytecode

## Invariants

1. **`Instruction` byte values are stable.** Changing them breaks existing
   bytecode. Add new instructions at the end.

2. **Signal inference is conservative.** See `signals/AGENTS.md`.

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Instruction` | `bytecode.rs` | Bytecode opcodes |
| `Bytecode` | `bytecode.rs` | Instructions + constants |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~10 | Re-exports |
| `bytecode.rs` | ~200 | Instruction enum, Bytecode struct |
| `bytecode_debug.rs` | ~150 | Debug formatting |

## Type guard instructions

Type guard instructions are used in pattern matching to check value types:

- `IsNil`, `IsEmptyList`, `IsPair`, `IsArray`, `IsArrayMut`, `IsStruct`, `IsStructMut`
  — check collection types
- `IsSet`, `IsSetMut` — check set types (immutable and mutable)
- `IsNumber`, `IsSymbol` — check scalar types

These instructions pop a value from the operand stack, check its type, and push
a boolean result. They are emitted by the pattern lowering logic when a `match`
expression has type guards.

## Parameter instructions

`PushParamFrame` and `PopParamFrame` manage dynamic parameter binding frames:

- `PushParamFrame(count: u8)` — Push a new parameter frame with `count` bindings
- `PopParamFrame` — Pop the current parameter frame

These are emitted by the lowerer for `parameterize` special forms. The VM
maintains a stack of parameter frames on the fiber. When a parameter is called,
the VM searches from the top of the stack downward for a binding, falling back
to the parameter's default value if no binding is found.

## Allocation region instructions

`RegionEnter` and `RegionExit` are scope boundary markers for the allocator.
They have no operands (single opcode byte each). In the VM, they push/pop
scope marks on the current FiberHeap (no-op for the root fiber). The lowerer
conditionally emits them based on escape analysis — currently maximally
conservative, so no region instructions are emitted. Function bodies never
get region instructions.

`break` emits compensating `RegionExit` instructions for each region entered
between the break site and the target block (`region_depth` tracking).

## Struct rest instruction

`StructRest` collects all keys from a struct NOT in an excluded set into a new
immutable struct. Operands: `u16 count` followed by `count` x `u16 const_idx`
(each a keyword or symbol key value in the constant pool). Source struct is
popped from the stack; result struct is pushed. Used by `{:a a & rest}`
destructuring patterns. The JIT side-exits on this instruction.

## Anti-patterns

- Modifying `Instruction` byte values (breaks compatibility)
- Adding compilation logic here (use `lir/` instead)
