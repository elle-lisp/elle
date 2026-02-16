# Resident Compiler

The resident compiler provides a persistent compilation service that caches
results for efficient re-use, primarily by the language server.

## Purpose

When editing Elle code, you don't want to recompile everything on each
keystroke. The resident compiler:

1. Caches compiled documents
2. Tracks dependencies between files
3. Invalidates only what's necessary on changes
4. Provides diagnostics for the editor

## Architecture

```
Source files
    │
    ▼
ResidentCompiler
    ├─► Check cache
    │   ├─► Hit: return cached
    │   └─► Miss: compile
    │           ├─► Parse
    │           ├─► Analyze
    │           ├─► Compile
    │           └─► Cache result
    │
    ▼
CompiledDocument
    ├─► bytecode
    ├─► diagnostics
    └─► source map
```

## Caching Strategy

- **In-memory**: Fast access during editing session
- **Disk** (`/dev/shm`): Survives editor restarts on Linux

Cache keys are based on source content hash, so identical files hit
the same cache entry.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `elle-lsp/` - primary consumer
- `src/pipeline.rs` - compilation pipeline used internally
