# LIR Lowering

<!-- audited: 2026-09-23 -->

Lowering turns HIR into LIR: it gives each binding a slot, builds capture cells, and turns control flow into jumps between blocks.

[AGENTS.md](AGENTS.md) holds what the `Lowerer` decides and the invariants it
keeps. This file maps the directory.

## Files

| File | Purpose |
|------|---------|
| [mod.rs](mod.rs) | The `Lowerer` and the state one function's lowering carries |
| [emitops.rs](emitops.rs) | Register and slot allocation, instruction emission, block management |
| [expr.rs](expr.rs) and [expr/](expr/) | The `lower_expr` dispatch and the expression forms |
| [binding.rs](binding.rs) and [binding/](binding/) | `let`, `letrec`, `def`, `var`, `assign` and destructuring |
| [lambda.rs](lambda.rs) and [lambda/](lambda/) | Closure construction and body compilation |
| [control.rs](control.rs) and [control/](control/) | `and`, `or`, `emit`, `eval`, calls and tail calls, tail-argument ownership |
| [tailcall.rs](tailcall.rs) | Whether a body ends in a tail call |
| [pattern.rs](pattern.rs) and [pattern/](pattern/) | A compiled decision tree lowered to blocks |
| [access.rs](access.rs) | Loading an access path for pattern matching |
| [aliases.rs](aliases.rs) | The bindings a pattern binds to a borrowed subview of its scrutinee |
| [regionemit.rs](regionemit.rs) and [regionemit/](regionemit/) | Region retains, cell stores and slot choices, from the solver's `RegionInfo` |
| [regiondecref.rs](regiondecref.rs) | Region releases |
| [naming.rs](naming.rs) | How a release names what it frees |
| [order.rs](order.rs) | The order of releases that share one `decref_point` |
| [relocate.rs](relocate.rs) | The relocation points a release still has to cover |
| [splice.rs](splice.rs) | Placing a release so every path runs it once |
| [rcstats.rs](rcstats.rs) | Compile-time RC-coalescing statistics |

## See also

- [../README.md](../README.md) — LIR and its two phases
- [../AGENTS.md](../AGENTS.md) — the LIR types and the emitter
