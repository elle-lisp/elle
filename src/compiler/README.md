# Compiler

<!-- audited: 2026-11-23 -->

The bytecode instruction set, the buffer the emitter writes it into, and the
disk cache for the compiled standard library.

The compiling happens elsewhere. `elle::pipeline::compile` takes source through
syntax, HIR and LIR to bytecode, and answers a `CodeUnit`
([pipeline/AGENTS.md](../pipeline/AGENTS.md)). Put compilation logic in
[lir](../lir/AGENTS.md), never here.

## What lives here

| Type | What it is |
|------|------------|
| `Instruction` | The bytecode opcodes |
| `Bytecode` | The emitter's working buffer: instructions, constants, the location map and the region tables |
| `StdlibCache` | Where a runtime caches its standard library: `Process`, `Dir`, or `Off` |
| `StoredBytecode` | The cache file's payload: the entry code object with its frozen LIR, the closures it reaches, and the spelling of every name |

The `Instruction` enum is defined in [instruction.rs](bytecode/instruction.rs),
where each opcode's doc comment names its operands and says what it does.
[bytecode.md](../../docs/impl/bytecode.md) holds the instruction set and its
encoding, and [disasm.rs](bytecode/disasm.rs) prints one line per instruction.

`StdlibCache` is a parameter of `Runtime::with_stdlib_cache`, never
process-global state. The suite builds many runtimes across threads, so each
one carries its own cache directory
([stdlib-cache](../../docs/impl/stdlib-cache.md)).

## The last variant

`MaterializeConst` is the last variant of `Instruction`, and a new instruction
goes before it ([the recipe](../../docs/cookbook/bytecode.md)). A byte value is
not stable across builds. The stdlib cache keys on the build, and an image's
fingerprint hashes the instruction set
([format.md](../../docs/impl/image/format.md)), so neither reads bytecode that a
renumbered build wrote.
