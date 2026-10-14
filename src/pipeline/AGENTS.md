# pipeline

<!-- audited: 2026-10-14 -->

The compilation entry points, from source text to bytecode, LIR, HIR or a value, and the compile context they share.

## Responsibility

- Run the stages in order for each entry point: read, strip declarations,
  migrate, expand, analyze, regularize, infer regions, lower, emit, and run
  when the entry point asks.
- Own `CompileCtx`, the compile context of one instance: the macro VM, the
  expander, the compile metadata of every global name, the REPL layer, and
  the store of the units loaded files compiled to.
- Boot the compile context: compile [core.lisp](../core.lisp) and load
  [prelude.lisp](../prelude.lisp), or install both from a boot image
  ([booting from an image](../../docs/impl/image/boot.md)).

Does NOT:
- Read source (that's `reader`)
- Expand macros (that's `syntax`)
- Resolve bindings or transform HIR (that's `hir`)
- Lower HIR or emit bytecode (that's `lir`)
- Execute bytecode (that's `vm`)

[src/hir/AGENTS.md](../hir/AGENTS.md) owns the passes `regularize` runs, the
shape of a file's letrec, and how primitives are bound.
[docs/pipeline.md](../../docs/pipeline.md) owns why each entry point expands
on the VM it does, the REPL layer, the store's key, and the signal fixpoint
over a file's letrec. [Grafting a call](../../docs/impl/graft.md) owns the
graft and the fold.

## Files

| File | Holds |
|------|-------|
| [mod.rs](mod.rs) | `AnalyzeResult`, and the re-exports |
| [compile.rs](compile.rs) | The bytecode and FHIR entry points, and the lowering they share |
| [compile/frontend.rs](compile/frontend.rs) | The file front end: source text or forms to regularized HIR |
| [compile/lir.rs](compile/lir.rs) | `compile_file_to_lir`: a file to frozen LIR |
| [compile/include.rs](compile/include.rs) | `include` and `include-file`: resolution, splicing, and `splice_includes` |
| [compile/transforms.rs](compile/transforms.rs) | The two test-runner transforms, and the entry points over them |
| [analyze.rs](analyze.rs) | `analyze` and `analyze_file` |
| [eval.rs](eval.rs) | `eval`, `eval_all`, `eval_file` and `eval_syntax` |
| [cache.rs](cache.rs) | `CompileCtx` and `BootExports` |
| [cache/register.rs](cache/register.rs) | What a host, a REPL line and the standard library register into a `CompileCtx` |
| [cache/repl.rs](cache/repl.rs) | The REPL layer: the macros and definitions earlier REPL lines made |
| [cache/store.rs](cache/store.rs) | The store, and `compile_file_stored` |
| [bootstrap.rs](bootstrap.rs) | The core.lisp compile, and `install_core_exports` |
| [sources.rs](sources.rs) | The three boot sources, embedded at build time |
| [directives.rs](directives.rs) | The `elle/version` and `elle/migration` declarations ([versioning](../../docs/versioning.md)) |

## Interface

Unless the table says otherwise, an entry point takes
`(source, symbols, cctx, source_name)` and returns `Result<_, String>`. The
`analyze` and `eval` families also take the caller's `vm`, between `symbols`
and `cctx`. `analyze`, `analyze_file` and `eval` expand on that VM, while
`eval_file` and `eval_all` only run the bytecode on it.

| Function | Answers | Purpose |
|----------|---------|---------|
| `compile` | `CodeUnit` | One form: read, expand, analyze, regularize, lower, emit |
| `compile_file` | `CodeUnit` | A file as one letrec, through the file front end. Sets the entry's signal |
| `compile_file_stored(…, path)` | `CodeUnit` | `compile_file` for a loader: the unit the store keeps for `path` while its compile inputs match |
| `compile_file_repl` | `(CodeUnit, Expander)` | `compile_file` over the REPL layer, and the expander, so the REPL keeps the macros a line defines |
| `compile_value(form, …)` | `CodeUnit` | One form, handed over as a datum or a syntax object, as a whole file |
| `compile_file_to_lir(…, epoch_skip)` | `FrozenModule` | A file to frozen LIR, for the WASM backend and the dumps. When `epoch_skip` is less than the form count, that many leading forms skip epoch migration |
| `compile_file_to_fhir` | `(Hir, BindingArena)` | A file to regularized HIR, for `--dump=fhir` |
| `compile_barrier_module` | `CodeUnit` | The test runner's per-form mode: the `def` and `var` forms run as the module runs, and each expression becomes an indexed thunk ([the test runner](../../docs/test-runner.md)) |
| `compile_whole_module` | `CodeUnit` | The test runner's whole-file mode: one thunk runs every form in source order |
| `compile_whole_module_forms(syntaxes, …)` | `CodeUnit` | `compile_whole_module` over forms that another VM read |
| `splice_includes(source, source_name, cctx)` | `String` | The source text with each `include` and `include-file` inlined, for the WASM backend |
| `analyze` | `AnalyzeResult` | One form to HIR, with tail calls marked. No regularize, no bytecode |
| `analyze_file` | `AnalyzeResult` | A file as one letrec to HIR, with tail calls marked. No regularize, no bytecode |
| `eval` | `Value` | One form, compiled and run on the caller's VM |
| `eval_file` | `Value` | `compile_file`, then `vm.execute` |
| `eval_all` | `Value` | `compile_file`, then `vm.execute_scheduled`, which runs the program inside `ev/run` as the binary does |
| `eval_syntax(syntax, expander, symbols, vm)` | `Value` | Compile and run a macro transformer or a `begin-for-syntax` value, with the caller's expander |
| `install_core_exports(exports_val, symbols, meta, expander)` | — | Put core.lisp's exports into the expander's `core_env` and into the compile metadata. Both boots end here |

| Type | Purpose |
|------|---------|
| `CodeUnit` | The entry's code object, and one counted reference to the code region every payload of the unit lives in ([template.md](../../docs/impl/region/template.md)) |
| `AnalyzeResult` | `hir`, its `arena`, the non-fatal `errors` the analyzer collected, and two facts the HIR does not keep: `lambda_decls`, each lambda's declared ceiling and the bits its body raises by itself, and `top_direct`, the bits the code outside every lambda raises by itself. A signal solver reads both ([the solver](../../docs/impl/solver.md)) |
| `CompileCtx` | The compile context of one instance ([cache.rs](cache.rs)) |
| `Layer` | Which layer a compile sees: `Instance`, or `Repl`, which adds the REPL layer |
| `BootExports` | The core and stdlib export structs a boot leaves, which a boot image carries as its roots |
| `directives::SemverMeta` | The version and the migration majors that `extract_semver_directives` removed from a file |
| `sources::{CORE, PRELUDE, STDLIB}` | [core.lisp](../core.lisp), [prelude.lisp](../prelude.lisp) and [stdlib.lisp](../stdlib.lisp), as text |

### The compile context

`CompileCtx` holds the macro VM, the expander, the compile metadata, the REPL
layer, the capabilities the fiber a compile runs for withholds, the boot
exports, the store, and the graft policy. Its methods fall into seven groups:

- **Build**: `new`, `new_with_heap` (the macro VM shares the instance heap),
  and `from_boot_image` (crate-internal).
- **Configure**: `set_graft` sets whether the units compiled from then on
  graft their calls, and `graft` reads it. Both are crate-internal. The
  owning `Runtime` calls `set_graft` after its boot, so the boot's own units
  always graft ([grafting a call](../../docs/impl/graft.md)). `on_behalf_of`
  runs a compile with the capabilities a fiber withholds, and `withheld`
  reads them.
- **Expand**: `with_macro_expansion` runs a closure on the macro VM, and
  `expander_and_meta` answers the clones alone. Both take the unit's syntax
  arena and its tag, and hand out clones of the expander and the compile
  metadata, the expander begun on the unit. `with_macro_expansion` also takes
  the `Layer`.
- **Register**: `register_host_binding` and `register_primitive` reach every
  compile in the instance. `register_stdlib_exports` installs the standard
  library's exports. `register_repl_binding` and `register_repl_macros` reach
  later REPL lines alone. Each registration that every compile sees empties
  the store.
- **Look up**: `lookup_stdlib_value`, `global_function_ids`, `meta`,
  `primitive_meta` and `core_env`.
- **Store**: `compile_file_stored` keeps each loaded file's unit while its
  compile inputs match ([docs/pipeline.md](../../docs/pipeline.md)).
- **Tear down**: `release` empties the store, and drops the references the
  cached macro transformers hold, the REPL layer's among them.

## Data flow

This is the path of `compile_file`. The test-runner modes and
`compile_file_to_fhir` share it through the file front end.

```text
source text
    │  read_syntax_all_for (the Lua surface for a .lua file)
    │  check_lexicon_agreement
    │  UnitTag::of: the unit's tag, from its name and its forms as read
    ▼
top-level forms
    │  extract_epoch, extract_semver_directives, migrate_forms
    ▼
current-epoch forms
    │  with_macro_expansion: stamp the file scope unless the name starts
    │  with <, splice includes, expand
    ▼
expanded forms
    │  the test-runner transform, in those two modes only
    │  classify_form: Def, Var, Signal or Expr
    ▼
Analyzer: bind_primitives, core_env, analyze_file_letrec
    │  the first collected error ends the compile, as file:line:col: message
    ▼
HIR: one Letrec
    │  hir::regularize, in this order:
    │    prune dead (type-of x) arms
    │    close every lambda into its fragment
    │    map fusion
    │    the graft
    │    dead-binding elimination
    │    tail marking
    │    functionalization
    │    the ANF lift
    │    type inference, the fold, and the operand proofs
    ▼
regularized HIR + TypeInfo ─────► compile_file_to_fhir stops here
    │  analyze_regions_with
    ▼
Lowerer ─► FrozenModule, built in a working region the lowerer frees
    │
    ▼
Emitter ─► CodeUnit: each payload written into the unit's code region,
           with the entry signal
    │
    ▼
eval_file and eval_all run it on the caller's VM
```

[regularize.rs](../hir/regularize.rs) says why the passes run in that order.
The other entry points leave this path:

- `compile_file_to_lir` repeats the front end in
  [compile/lir.rs](compile/lir.rs). It stamps no file scope, honors
  `epoch_skip`, and stops at LIR.
- `analyze_file` reads with `read_syntax_all`, strips the declarations,
  migrates and expands. It checks no lexicon, stamps no file scope, splices no
  include, and binds no `core_env`. It then analyzes the letrec and marks tail
  calls.
- `compile`, `analyze` and `eval` read one form with `read_syntax`, and skip
  the declarations and the epoch migration.
- `eval` and `eval_syntax` do not regularize. They mark tail calls,
  functionalize and ANF-lift, then lower with no type information.
- The core.lisp bootstrap expands with a bare expander that has no prelude
  macros, then regularizes, lowers and runs the result. Any failure there
  panics.

## Dependents

- [program.rs](../program.rs) — runs a file, stdin or `-e` through
  `compile_file`
- [runtime/core.rs](../runtime/core.rs) — builds the instance's compile
  context with `new_with_heap` or `from_boot_image`
- [primitives/module_init.rs](../primitives/module_init.rs) — compiles
  stdlib.lisp through `compile_file`, then calls `register_stdlib_exports`
- [primitives/modules.rs](../primitives/modules.rs) — compiles a loaded file
  through `compile_file_stored`, on behalf of the calling fiber
- [repl/eval.rs](../repl/eval.rs) and [repl/defer.rs](../repl/defer.rs) —
  compile each input with `compile_file_repl`, then register its definitions
  and macros
- [lsp/state.rs](../lsp/state.rs) — `analyze_file`, and `global_function_ids`
  for completion
- [lint/cli.rs](../lint/cli.rs) — `analyze_file`
- [primitives/compile/query/analysis.rs](../primitives/compile/query/analysis.rs)
  — `analyze_file`, for `compile/analyze`
- [vm/signal/modules.rs](../vm/signal/modules.rs) — the three test-runner
  entry points, for `compile/barrier-module`, `compile/whole-module` and
  `compile/whole-module-syntax`
- [dump/mod.rs](../dump/mod.rs) —
  `compile_file_to_fhir` and `compile_file_to_lir`, for `--dump`
- [wasm/mod.rs](../wasm/mod.rs) — `splice_includes` and `compile_file_to_lir`
- [primitives/concurrency/worker.rs](../primitives/concurrency/worker.rs) — a
  compile context for each worker thread
- [syntax/expand/macro_expand.rs](../syntax/expand/macro_expand.rs) and
  [syntax/expand/compiletime.rs](../syntax/expand/compiletime.rs) —
  `eval_syntax`
- [syntax/expand/mod.rs](../syntax/expand/mod.rs) — `sources::PRELUDE`, for
  `load_prelude`
- [vm/eval.rs](../vm/eval.rs) — `core_env` and `primitive_meta`, for the
  runtime `eval`
- [vm/scheduled.rs](../vm/scheduled.rs) — `lookup_stdlib_value`, to find `ev/run`
- [image/boot/](../image/boot/) — hashes the three sources, and dumps the
  boot exports and the macro table into an image
- [tests/common/mod.rs](../../tests/common/mod.rs) — `eval_all`

## Invariants

1. **Each instance owns one compile context, passed as a parameter.** The
   `RuntimeCore` owns it beside the VM and the symbol table. Two instances on
   one thread see neither the other's stdlib exports, its REPL definitions,
   nor its stored units.

2. **A call expands with clones.** `with_macro_expansion` and
   `expander_and_meta` clone the expander and the compile metadata, so a
   nested compile during expansion does not alias the compile context. The
   clone has begun the unit the compile expands. A macro that one compile
   defines reaches a later compile only through `register_repl_macros`, and
   only a REPL line, compiled through `compile_file_repl`, sees it.

3. **A unit reads and expands in its own syntax arena.** Each entry point
   except `eval_syntax` mints the arena and frees it on return, and nothing in
   the result points into it ([the syntax tree](../../docs/impl/syntax.md)).
   `eval_syntax` expands in the arena of the expander that calls it.

4. **Every name in the compile metadata is bound before analysis.** Each entry
   point calls `bind_primitives` with its compile metadata. For a user compile
   that holds the primitives, the core.lisp and stdlib exports, and the host
   bindings, and for a REPL line the REPL definitions too. `eval_syntax` binds
   the macro-body metadata instead, which holds the primitives and the stdlib
   exports.

5. **A file is one letrec.** Every file entry point classifies the expanded
   forms and analyzes them with `analyze_file_letrec`.

6. **Version and migration declarations leave before epoch migration.**
   `extract_semver_directives` validates and removes them, so an epoch rewrite
   never reaches inside one.

7. **Every lowering path except `eval` and `eval_syntax` regularizes.** The
   `TypeInfo` that `regularize` answers goes to the lowerer, which marks the
   operations whose operands inference proved integers.

8. **A compile never compiles another file.** The analyzer reads nothing from
   the target of an `import-file`. The loader compiles that file when the
   import runs, so a cycle of imports is a cycle of loads, which the loader
   reports.

9. **A loader compiles a file once while its inputs match.**
   `compile_file_stored` keeps each unit in the instance's store under the
   file's path, beside a digest of the compile inputs that can change. A load
   whose digest matches runs a clone of the kept unit. Registering a host
   binding, a host primitive or the standard library's exports empties the
   store ([docs/pipeline.md](../../docs/pipeline.md)).
