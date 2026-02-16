# resident_compiler

Persistent compilation service. Caches compiled code for LSP and CLI.

## Responsibility

- Cache compiled expressions (memory and disk)
- Maintain symbol tables across compilations
- Track source locations for diagnostics
- Support incremental compilation

Does NOT:
- Implement compilation (uses `pipeline`, `compiler`)
- Provide language server protocol (that's `elle-lsp`)
- Execute code (that's `vm`)

## Interface

| Type | Purpose |
|------|---------|
| `ResidentCompiler` | Main compilation interface |
| `CompiledDocument` | Cached compilation result |

## Usage

```rust
let mut compiler = ResidentCompiler::new();

// Compile a document
let doc = compiler.compile_document(uri, source)?;

// Access compiled form
let bytecode = doc.bytecode();
let diagnostics = doc.diagnostics();
```

## Dependents

- `elle-lsp` - uses for compilation
- CLI - potential future use

## Caching

Compiled documents are cached:
- In-memory for fast access
- Optionally on disk (`/dev/shm`) for persistence

Cache invalidation occurs when source changes.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 15 | Re-exports |
| `compiler.rs` | ~200 | `ResidentCompiler` |
| `compiled_doc.rs` | ~150 | `CompiledDocument` |
| `cache.rs` | ~100 | Caching logic |
