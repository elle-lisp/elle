# Elle Core Crate

The main Elle interpreter and compiler. This crate implements the complete pipeline from source code to bytecode execution on a register-based virtual machine.

## Architecture Overview

```
Source → Reader → Syntax → Expander → Analyzer → Lowerer → Emitter → Bytecode → VM
```

Source locations flow through the entire pipeline, enabling precise error messages with file:line:col information.

## Key Modules

| Module | Purpose |
|--------|---------|
| `reader` | Lexing and parsing S-expressions to `Syntax` trees |
| `syntax` | Syntax types, macro expansion, hygiene via scope sets |
| `hir` | Binding resolution, capture analysis, signal inference |
| `lir` | SSA form with virtual registers and basic blocks |
| `compiler` | Bytecode instruction definitions and debug formatting |
| `vm` | Bytecode execution engine and builtin documentation |
| `value` | Runtime value representation (NaN-boxed 8-byte values): LArray, LArrayMut, LStruct, LStructMut, LString, LStringMut, LBytes, LBytesMut, LSet, LSetMut |
| `signals` | Signal type system (`Signal` struct with `bits` and `propagates`) |
| `lint` | Diagnostic types and static analysis rules |
| `symbols` | Symbol table and IDE feature support |
| `error` | Error types and source location mapping |
| `formatter` | Code formatting for Elle source |
| `ffi` | C interop via libloading and bindgen |
| `jit` | JIT compilation via Cranelift for non-suspending functions |
| `lsp` | Language server protocol implementation |
| `rewrite` | Source-to-source rewriting |
| `primitives` | Built-in functions (arithmetic, list, string, I/O, etc.) |
| `pipeline` | Compilation entry points (`compile`, `analyze`, `eval`) |

## Where to Start

1. Read [`pipeline/mod.rs`](pipeline/mod.rs) — shows the full compilation flow in ~50 lines
2. Read an example in [`examples/`](../examples/) to understand the surface syntax
3. Read [`value/mod.rs`](value/mod.rs) to understand runtime representation
4. Read a failing test to understand what's expected
5. Read the [AGENTS.md](AGENTS.md) in the specific module you're working on

## Key Invariants

1. **Bindings are resolved at analysis time.** HIR contains `Binding` (NaN-boxed Value), not symbols.
2. **Closures capture by value into their environment.** Mutable captures use `LocalLBox`.
3. **Signals are inferred, not declared.** The `Signal` type propagates from leaves to root during analysis.
4. **The VM is stack-based for operands, register-addressed for locals.** Instructions reference registers by index.
5. **Errors propagate.** Functions return `LResult<T>`. Silent failure is forbidden.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`docs/pipeline.md`](../docs/pipeline.md) - detailed compilation pipeline documentation
- [`examples/`](../examples/) - executable semantics documentation
