# Compiler

<!-- audited: 2026-11-14 -->

The bytecode instruction set, the buffer the emitter writes it into, and the
disk cache for the compiled standard library.

The compiling happens elsewhere. `elle::pipeline::compile` takes source through
syntax, HIR and LIR to bytecode, and answers a `CodeUnit`
([pipeline/AGENTS.md](../pipeline/AGENTS.md)).
[bytecode.md](../../docs/impl/bytecode.md) holds the instruction set and its
encoding, and [AGENTS.md](AGENTS.md) names the files here.
