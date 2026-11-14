# compiler

<!-- audited: 2026-11-14 -->

Bytecode instruction definitions, debug formatting, and the stdlib disk cache.

## Responsibility

- Define the `Instruction` enum (bytecode opcodes)
- Define the `Bytecode` struct, the emitter's working buffer
- Disassemble bytecode for debugging
- Persist and restore the compiled standard library

[bytecode.md](../../docs/impl/bytecode.md) holds the instruction set and its
encoding. Each opcode's doc comment in
[instruction.rs](bytecode/instruction.rs) names its operands and says what it
does.

## Files

| File | Purpose |
|------|---------|
| [bytecode.rs](bytecode.rs) | `Bytecode`, the emit and patch surface, disassembly entry points |
| [instruction.rs](bytecode/instruction.rs) | `Instruction` enum, opcode decoding |
| [disasm.rs](bytecode/disasm.rs) | One line per instruction, with its operands decoded |
| [stdlib_cache.rs](stdlib_cache.rs) | Disk cache for the compiled stdlib ([stdlib-cache.md](../../docs/impl/stdlib-cache.md)) |

## Dependents

- [pipeline/](../pipeline/AGENTS.md) - hands out the compiled `CodeUnit`
- [lir/emit/](../lir/emit/mod.rs) - writes `Instruction` bytes into a `Bytecode`
- [vm/](../vm/AGENTS.md) - executes bytecode

## Invariants

1. **`MaterializeConst` is the last variant.** A new instruction goes before it
   ([the recipe](../../docs/cookbook/bytecode.md)). A byte
   value is not stable across builds. The stdlib cache keys on the binary
   itself, and an image's fingerprint hashes the instruction set
   ([format.md](../../docs/impl/image/format.md)), so neither reads bytecode a
   renumbered build wrote.

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Instruction` | [instruction.rs](bytecode/instruction.rs) | Bytecode opcodes |
| `Bytecode` | [bytecode.rs](bytecode.rs) | Instructions, constants, location map and the region tables |
| `StdlibCache` | [stdlib_cache.rs](stdlib_cache.rs) | Where a runtime caches its stdlib: `Process`, `Dir`, or `Off`. A construction parameter, passed to `Runtime::with_stdlib_cache` — never read from process-global state |
| `StoredBytecode` | [stdlib_cache.rs](stdlib_cache.rs) | The cache file's payload: fully owned data, symbols by name, LIR preserved |

## Anti-patterns

- Adding compilation logic here (use `lir/` instead)
