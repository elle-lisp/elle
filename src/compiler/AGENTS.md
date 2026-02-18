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

2. **Effect inference is conservative.** Unknown calls are `IO`. Only proven
   pure code is `Pure`.

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

## Anti-patterns

- Modifying `Instruction` byte values (breaks compatibility)
- Adding compilation logic here (use `lir/` instead)
