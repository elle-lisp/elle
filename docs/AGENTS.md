# docs

Design documents, language references, and contributor guides for Elle.

## Navigation Index

### Language Reference

| File | Description | Referenced from |
|------|-------------|-----------------|
| `language.md` | Complete language reference: syntax, data types, variables, functions, control flow, scoping | Root AGENTS.md |
| `types.md` | Type system: mutable/immutable split, all types, predicates, display format, equality | Root AGENTS.md |
| `semantics.md` | Authoritative semantics: truthiness, lists, conditionals, equality, destructuring | Root AGENTS.md |
| `macros.md` | Macro system: current state, architecture, hygiene, scope sets | Root AGENTS.md |
| `signals.md` | Signal system design: fiber signals, error signalling, user-defined signals, JIT integration | Root AGENTS.md |
| `modules.md` | Module system: closure-as-module, parametric imports, qualified symbols, trade-offs | Root AGENTS.md |

### Contributor Guides

| File | Description | Referenced from |
|------|-------------|-----------------|
| `cookbook.md` | Step-by-step recipes for common changes: new primitives, heap types, bytecode instructions, special forms, lint rules, macros | Root AGENTS.md |
| `testing.md` | Testing strategy: decision tree, test categories, property tests, CI structure, running tests | Root AGENTS.md |
| `pipeline.md` | Compilation pipeline: entry points, VM ownership, expander lifecycle, fixpoint loop, caching | Root AGENTS.md |
| `embedding.md` | Embedding Elle as a library: Rust/C hosts, step-based scheduler, custom primitives | Root AGENTS.md |
| `debugging.md` | Debugging toolkit: introspection primitives, time API, signal system, memory profiling | Root AGENTS.md |
| `oddities.md` | Intentional design choices that look wrong: nil vs empty list, comment/splice syntax, mutation, collection literals | Root AGENTS.md |

### Design Documents

| File | Description | Referenced from |
|------|-------------|-----------------|
| `signals.md` | Signal system design: motivation, signal protocol, error signalling, signal inference, JIT integration | Root AGENTS.md |
| `fibers.md` | Fiber architecture: execution contexts, signals, suspension/resumption, parent/child chains | Root AGENTS.md |
| `ffi.md` | FFI design: type descriptors, signatures, calling C functions, callbacks, marshalling | Root AGENTS.md |

### Implementation Backends (`impl/`)

| File | Description |
|------|-------------|
| `impl/reader.md` | Lexer, parser, source locations, markdown literate mode |
| `impl/hir.md` | Binding resolution, signal inference |
| `impl/lir.md` | SSA form, virtual registers, basic blocks |
| `impl/bytecode.md` | Instruction set and encoding |
| `impl/vm.md` | Stack machine, dispatch loop, fiber integration |
| `impl/jit.md` | Cranelift JIT compilation |
| `impl/wasm.md` | WebAssembly backend (Wasmtime) |
| `impl/mlir.md` | MLIR/LLVM tier-2 CPU backend |
| `impl/spirv.md` | SPIR-V emission (compiler-generated + hand-written DSL) |
| `impl/gpu.md` | End-to-end GPU compute (MLIR + SPIR-V + Vulkan) |
| `impl/differential.md` | Cross-tier agreement harness (`compile/run-on`) |
| `impl/values.md` | Value representation, tagged union |

### Reference Material

| File | Description | Status |
|------|-------------|--------|
| `reference/janet.md` | Janet language reference | External design inspiration, not Elle's implementation |
| `reference/janet-compiler.md` | Janet compiler design | External design inspiration, not Elle's implementation |
| `reference/janet-destructuring.md` | Janet destructuring patterns | External design inspiration, not Elle's implementation |

## Important Notes

### docs/reference/

The `reference/` subdirectory contains external design inspiration from Janet and other Lisps. These are **NOT** Elle's implementation — they are reference materials that informed Elle's design. When reading these files, remember:

- These describe other languages, not Elle
- Elle may differ in implementation details
- For Elle's actual behavior, consult the main docs directory
- For Elle's implementation, read the source code and AGENTS.md files in `src/`

### Adding New Documentation

When you add a new doc file:
1. Add it to this index (AGENTS.md) with a one-line description
2. Add it to [README.md](README.md) in the appropriate section
3. If it's referenced from root AGENTS.md, mark it with ★ in the "Referenced from" column
4. If it's a design document or reference material, create a new section if needed

### Safe to Delete

The following files are safe to delete if they become stale:
- `reference/` files (external inspiration, not authoritative)
- Any file not referenced from root AGENTS.md or cookbook.md

### Authoritative Documents

These documents are authoritative and should be kept in sync with implementation:
- `language.md` — language reference
- `types.md` — type system
- `semantics.md` — semantic definitions
- `signals.md` — signal system design
- `fibers.md` — fiber architecture
- `ffi.md` — FFI design
- `pipeline.md` — compilation pipeline
- `testing.md` — testing strategy
- `cookbook.md` — recipes for common changes
- `modules.md` — module system design

When code contradicts these documents, **update the document** (or file an issue if the code is wrong).

## Cross-References

### From Root AGENTS.md

Root AGENTS.md references these docs:
- `pipeline.md` — compilation pipeline architecture
- `language.md` — language reference
- `types.md` — type system
- `signals.md` — signal system and signal inference
- `fibers.md` — fiber concurrency
- `macros.md` — macro system
- `ffi.md` — foreign function interface
- `signals.md` — signal system and error signalling
- `semantics.md` — semantic details
- `testing.md` — testing strategy
- `debugging.md` — debugging tools
- `cookbook.md` — recipes for common changes

### From cookbook.md

`cookbook.md` references:
- `language.md` — for syntax examples
- `types.md` — for type system details
- `signals.md` — for signal annotations
- `testing.md` — for test organization

## Files

| File | Lines | Content |
|------|-------|---------|
| `README.md` | ~60 | Human-facing entry point with grouped table of contents |
| `AGENTS.md` | ~150 | Agent-facing navigation index with cross-references |
| `language.md` | 1356 | Complete language reference |
| `types.md` | 444 | Type system definition |
| `semantics.md` | 154 | Authoritative semantics |
| `macros.md` | 334 | Macro system design |
| `signals.md` | ~1200 | Signal system design and error signalling |
| `cookbook.md` | 647 | Recipes for common changes |
| `testing.md` | 472 | Testing strategy |
| `pipeline.md` | 256 | Compilation pipeline |
| `config.md` | ~150 | Runtime configuration: vm/config API, trace keywords, JIT/WASM policies, CLI sugar |
| `debugging.md` | 220 | Debugging toolkit |
| `oddities.md` | 280 | Intentional design oddities |
| `fibers.md` | 312 | Fiber architecture |
| `ffi.md` | 455 | FFI design |
| `modules.md` | ~240 | Module system design |
| `reference/janet.md` | ~200 | Janet language reference |
| `reference/janet-compiler.md` | ~150 | Janet compiler design |
| `reference/janet-destructuring.md` | ~100 | Janet destructuring patterns |
