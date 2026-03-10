# src

Core interpreter and compiler crate. Implements the full Elle pipeline from source to bytecode execution.

## Responsibility

Provide the complete Elle implementation:
- Parse S-expressions and expand macros
- Analyze code for bindings, captures, and effects
- Lower to intermediate representations
- Emit bytecode
- Execute bytecode on a register-based VM
- Provide built-in functions and FFI support

## Top-level files

| File | Purpose |
|------|---------|
| `lib.rs` | Public API exports, crate documentation |
| `main.rs` | CLI entry point (REPL, file execution, lint, LSP, rewrite) |
| `arithmetic.rs` | Unified arithmetic operations (shared by VM and primitives) |
| `context.rs` | Thread-local VM and symbol table context |
| `plugin.rs` | Dynamic plugin loading for `.so` cdylib crates |
| `path.rs` | UTF-8 path operations (wraps camino, path-clean, pathdiff) |

## Module structure

| Module | Purpose |
|--------|---------|
| `reader` | Lexing and parsing to `Syntax` |
| `syntax` | Syntax types, macro expansion |
| `hir` | Binding resolution, capture analysis, effect inference, linting |
| `lir` | SSA form with virtual registers, basic blocks, source tracking |
| `compiler` | Bytecode instruction definitions and debug formatting |
| `vm` | Bytecode execution, builtin documentation storage |
| `value` | Runtime value representation (NaN-boxed) with types: LArray, LArrayMut, LStruct, LStructMut, LString, LStringMut, LBytes, LBytesMut, LSet, LSetMut |
| `effects` | Effect type system (`Inert`, `Yields`, `Polymorphic`) |
| `lint` | Diagnostic types and lint rules |
| `symbols` | Symbol index types for IDE features |
| `error` | Error types and source location mapping |
| `formatter` | Code formatting for Elle source |
| `ffi` | C interop via libloading/bindgen |
| `jit` | JIT compilation via Cranelift for non-suspending functions |
| `lsp` | Language server protocol implementation |
| `rewrite` | Source-to-source rewriting |
| `primitives` | Built-in functions (arithmetic, list, string, I/O, concurrency, etc.) |
| `pipeline` | Compilation entry points (`compile`, `analyze`, `eval`) |
| `repl` | Interactive REPL |
| `symbol` | Symbol table and interning |
| `port` | I/O port abstraction |

## Compilation pipeline

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

Source locations flow through the entire pipeline: Syntax spans → HIR spans → LIR `SpannedInstr` → `LocationMap` in bytecode. Error messages include file:line:col information.

## Where to start

1. Read `pipeline/mod.rs` — shows the full compilation flow in ~50 lines
2. Read an example in `examples/` to understand the surface syntax
3. Read `value/mod.rs` to understand runtime representation
4. Read a failing test to understand what's expected
5. Read the AGENTS.md in the specific module you're working on

## Key invariants

1. **Bindings are resolved at analysis time.** HIR contains `Binding` (NaN-boxed Value), not symbols.
2. **Closures capture by value into their environment.** Mutable captures use `LocalCell`.
3. **Effects are inferred, not declared.** The `Effect` enum propagates from leaves to root during analysis.
4. **The VM is stack-based for operands, register-addressed for locals.** Instructions reference registers by index.
5. **Errors propagate.** Functions return `LResult<T>`. Silent failure is forbidden.

## Dependents

- `main.rs` — CLI entry point
- `repl.rs` — Interactive REPL
- `tests/` — Comprehensive test suite
- `examples/` — Executable semantics documentation
- `plugins/` — Dynamically-loaded plugin crates

## Files

| File | Lines | Content |
|------|-------|---------|
| `lib.rs` | 75 | Public API exports |
| `main.rs` | 433 | CLI entry point |
| `arithmetic.rs` | 270 | Unified arithmetic operations |
| `context.rs` | 56 | Thread-local VM/symbol table context |
| `plugin.rs` | 102 | Plugin loading |
| `path.rs` | 359 | UTF-8 path operations |
