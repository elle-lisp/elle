# Elle Documentation

This directory contains design documents, language references, and contributor guides for Elle.

## Language Reference

| File | Description |
|------|-------------|
| [language.md](language.md) | Complete language reference: syntax, data types, variables, functions, control flow, scoping |
| [types.md](types.md) | Type system: mutable/immutable split, all types, predicates, display format, equality |
| [semantics.md](semantics.md) | Authoritative semantics: truthiness, lists, conditionals, equality, destructuring |
| [macros.md](macros.md) | Macro system: current state, architecture, hygiene, scope sets |
| [except.md](except.md) | Exception handling: error structs, try/catch, error propagation |

## Contributor Guides

| File | Description |
|------|-------------|
| [cookbook.md](cookbook.md) | Step-by-step recipes for common changes: new primitives, heap types, bytecode instructions, special forms, lint rules, macros |
| [testing.md](testing.md) | Testing strategy: decision tree, test categories, property tests, CI structure, running tests |
| [pipeline.md](pipeline.md) | Compilation pipeline: entry points, VM ownership, expander lifecycle, fixpoint loop, caching |
| [debugging.md](debugging.md) | Debugging toolkit: introspection primitives, time API, effect system, memory profiling |

## Design Documents

| File | Description |
|------|-------------|
| [effects.md](effects.md) | Effect system design: motivation, signal protocol, effect inference, JIT integration |
| [fibers.md](fibers.md) | Fiber architecture: execution contexts, signals, suspension/resumption, parent/child chains |
| [ffi.md](ffi.md) | FFI design: type descriptors, signatures, calling C functions, callbacks, marshalling |

## Reference Material

| File | Description |
|------|-------------|
| [reference/janet.md](reference/janet.md) | Janet language reference (design inspiration, not Elle's implementation) |
| [reference/janet-compiler.md](reference/janet-compiler.md) | Janet compiler design (reference material) |
| [reference/janet-destructuring.md](reference/janet-destructuring.md) | Janet destructuring patterns (reference material) |

## Quick Navigation

- **Starting out?** Read [language.md](language.md) first, then [pipeline.md](pipeline.md)
- **Adding a feature?** Check [cookbook.md](cookbook.md) for the recipe
- **Understanding effects?** Read [effects.md](effects.md)
- **Working with concurrency?** Read [fibers.md](fibers.md)
- **Implementing FFI?** Read [ffi.md](ffi.md)
- **Writing tests?** Read [testing.md](testing.md)
- **Debugging?** See [debugging.md](debugging.md)

## Maintaining Documentation

When you change a module's interface or discover undocumented behavior:
1. Update the relevant doc file
2. Update the module's AGENTS.md
3. If adding a new doc, add it to this index and to [AGENTS.md](AGENTS.md)

Documentation debt compounds. A few minutes now saves hours of confusion later.
