# Elle Documentation

This directory contains language references, design documents, and contributor
guides. See [QUICKSTART.md](../QUICKSTART.md) for the full table of contents.

## Language Topics

Focused files covering one topic each, all runnable via `elle docs/<file>.md`.

`syntax` `types` `bindings` `destructuring` `destructuring-advanced`
`functions` `named-args` `arrays` `structs` `sets` `strings` `bytes`
`control` `loops` `match` `errors` `concurrency` `threads` `coroutines`
`parameters` `traits` `io` `lua` `epochs`

## Design Documents

| Directory | Content |
|-----------|---------|
| [signals/](signals/) | Signal system design, protocol, inference, JIT |
| [signals/fibers.md](signals/fibers.md) | Fiber architecture |

## Contributor Guides

| Directory | Content |
|-----------|---------|
| [cookbook/](cookbook/) | Recipes: primitives, heap types, bytecode, plugins |
| [analysis/](analysis/) | Testing strategy, debugging, portraits |
| [pipeline.md](pipeline.md) | Compilation pipeline |

## Implementation

| Directory | Content |
|-----------|---------|
| [impl/](impl/) | Reader, HIR, LIR, bytecode, VM, JIT, values |

## Reference

| File | Content |
|------|---------|
| [plugins.md](plugins.md) | 29 shipped plugins |
| [stdlib.md](stdlib.md) | Standard library and prelude |
| [modules.md](modules.md) | Import system |
| [macros.md](macros.md) | Macro system |
| [ffi.md](ffi.md) | C interop |

## Quick Navigation

- **Starting out?** Read [QUICKSTART.md](../QUICKSTART.md)
- **Adding a feature?** Check [cookbook/](cookbook/)
- **Understanding signals?** Read [signals/](signals/)
- **Writing tests?** Read [analysis/](analysis/)
