# Compiler

<!-- audited: 2026-11-26 -->

The bytecode instruction set, and the buffer the emitter writes it into.

The compiling happens elsewhere. `elle::pipeline::compile` takes source through
syntax, HIR and LIR to bytecode, and answers a `CodeUnit`
([the pipeline overview](../pipeline/overview.md)). Put compilation logic in
[lir](../lir/AGENTS.md), never here.

## What lives here

| Type | What it is |
|------|------------|
| `Instruction` | The bytecode opcodes |
| `Bytecode` | The emitter's working buffer: instructions, constants, the location map and the region tables |

The `Instruction` enum is defined in [instruction.rs](bytecode/instruction.rs),
where each opcode's doc comment names its operands and says what it does.
[bytecode.md](../../docs/impl/bytecode.md) holds the instruction set and its
encoding, and [disasm.rs](bytecode/disasm.rs) prints one line per instruction.

## The last variant

`MaterializeConst` is the last variant of `Instruction`, and a new instruction
goes before it ([the recipe](../../docs/cookbook/bytecode.md)). A byte value is
not stable across builds. An image's fingerprint hashes the instruction set
([format.md](../../docs/impl/image/format.md)), so no build hydrates an image
whose bytecode a renumbered build wrote.
