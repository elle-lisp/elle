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
| `except.md` | Exception handling: error tuples, try/catch, error propagation | Root AGENTS.md |

### Contributor Guides

| File | Description | Referenced from |
|------|-------------|-----------------|
| `cookbook.md` | Step-by-step recipes for common changes: new primitives, heap types, bytecode instructions, special forms, lint rules, macros | Root AGENTS.md |
| `testing.md` | Testing strategy: decision tree, test categories, property tests, CI structure, running tests | Root AGENTS.md |
| `pipeline.md` | Compilation pipeline: entry points, VM ownership, expander lifecycle, fixpoint loop, caching | Root AGENTS.md |
| `debugging.md` | Debugging toolkit: introspection primitives, time API, effect system, memory profiling | Root AGENTS.md |

### Design Documents

| File | Description | Referenced from |
|------|-------------|-----------------|
| `effects.md` | Effect system design: motivation, signal protocol, effect inference, JIT integration | Root AGENTS.md |
| `fibers.md` | Fiber architecture: execution contexts, signals, suspension/resumption, parent/child chains | Root AGENTS.md |
| `ffi.md` | FFI design: type descriptors, signatures, calling C functions, callbacks, marshalling | Root AGENTS.md |

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
- `effects.md` — effect system design
- `fibers.md` — fiber architecture
- `ffi.md` — FFI design
- `pipeline.md` — compilation pipeline
- `testing.md` — testing strategy
- `cookbook.md` — recipes for common changes

When code contradicts these documents, **update the document** (or file an issue if the code is wrong).

## Cross-References

### From Root AGENTS.md

Root AGENTS.md references these docs:
- `pipeline.md` — compilation pipeline architecture
- `language.md` — language reference
- `types.md` — type system
- `effects.md` — effect system
- `fibers.md` — fiber concurrency
- `macros.md` — macro system
- `ffi.md` — foreign function interface
- `except.md` — exception handling
- `semantics.md` — semantic details
- `testing.md` — testing strategy
- `debugging.md` — debugging tools
- `cookbook.md` — recipes for common changes

### From cookbook.md

`cookbook.md` references:
- `language.md` — for syntax examples
- `types.md` — for type system details
- `effects.md` — for effect annotations
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
| `except.md` | 196 | Exception handling |
| `cookbook.md` | 647 | Recipes for common changes |
| `testing.md` | 455 | Testing strategy |
| `pipeline.md` | 256 | Compilation pipeline |
| `debugging.md` | 206 | Debugging toolkit |
| `effects.md` | 761 | Effect system design |
| `fibers.md` | 312 | Fiber architecture |
| `ffi.md` | 455 | FFI design |
| `reference/janet.md` | ~200 | Janet language reference |
| `reference/janet-compiler.md` | ~150 | Janet compiler design |
| `reference/janet-destructuring.md` | ~100 | Janet destructuring patterns |
