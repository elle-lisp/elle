# pipeline

<!-- audited: 2026-09-29 -->

Compilation entry points: source text to bytecode, or to HIR for a reader that wants the analysis alone.

[docs/pipeline.md](../../docs/pipeline.md) owns the entry points: their
signatures, which VM each expands macros on, the `Expander` each clones, and
the fixpoint loop.

## Responsibility

Orchestrate the full compilation pipeline:
- Reader: source text → Syntax
- Expander: Syntax → expanded Syntax (macro expansion)
- Analyzer: expanded Syntax → HIR (binding resolution, signal inference)
- Lowerer: HIR → LIR (register allocation, basic blocks)
- Emitter: LIR → Bytecode (instruction encoding)
- VM: Bytecode → Value (the `eval` family only)

Does NOT:
- Parse source (that's `reader`)
- Expand macros (that's `syntax`)
- Analyze bindings (that's `hir`)
- Generate code (that's `lir` and `compiler`)
- Execute bytecode (that's `vm`)

## Data flow

```
Source text
    │
    ▼
Reader (read_syntax / read_syntax_all)
    │
    ▼
Syntax (one or many)
    │
    ├─► check_lexicon_agreement (the declaration matches the lexer that
    │   read it — docs/impl/lexicon.md)
    │
    ├─► extract_epoch + migrate_forms (old-epoch syntax → current)
    │
    ▼
Expander (macro expansion, cached VM)
    │
    ▼
Expanded Syntax (one or many)
    │
    ▼
compile_file / analyze_file / eval_file
    │
    ▼
Analyzer.bind_primitives (wrap in primitive scope)
    │
    ▼
Analyzer.analyze_file_letrec (synthetic letrec)
    │
    ▼
HIR (single Letrec node)
    │
    ▼
Lowerer (HIR → LIR)
    │
    ▼
Emitter (LIR → Bytecode)
    │
    ▼
Bytecode
    │
    ▼
VM (execution)
    │
    ▼
Value
```

## File-as-letrec model

Files compile to a **single compilation unit**:

1. **Expand all forms** — macro expansion is idempotent
2. **Classify forms** — each form is `Def` (immutable), `Var` (mutable), or `Expr` (gensym-named)
3. **Bind primitives** — `Analyzer.bind_primitives` wraps the letrec in an outer scope
    containing all registered primitives as immutable Global bindings
4. **Analyze as letrec** — `Analyzer.analyze_file_letrec` does two-pass analysis:
    - Pass 1: pre-bind all names (enables mutual recursion)
    - Pass 2: analyze initializers sequentially
5. **Lower and emit** — standard LIR → Bytecode pipeline

Properties:
- Single `CompileResult` per file
- All forms analyzed together (mutual recursion works via pre-binding)
- Primitives are lexical bindings with compile-time immutability checks
- File's last expression is the return value

## Dependents

- `program.rs` — file, stdin and `-e` execution use `compile_file`
- `primitives/modules.rs` — `import-file` uses `compile_file`
- `primitives/module_init.rs` — the stdlib load uses `compile_file`
- `repl/` — each prompt form compiles through `compile_file_repl`
- `lsp/state.rs` and `lint/cli.rs` — file analysis uses `analyze_file`
- `tests/common/mod.rs` — test helpers use `eval_all`

## Invariants

1. **`compile` and `eval` are single-form entry points.** They parse a single
   expression, expand it, analyze it, and compile or execute it.

2. **`compile_file`, `analyze_file` and `eval_file` are file-level entry
   points.** They parse all top-level forms, expand them, classify them, and
   analyze them as a single synthetic letrec via
   `Analyzer.analyze_file_letrec`.

3. **`eval_all` delegates to `compile_file`.** It compiles the source as a
   single letrec, then executes it. Test helpers use it.

4. **Primitives are pre-bound in file-level analysis.** `Analyzer.bind_primitives`
   wraps the file's letrec in an outer scope containing all registered primitives.
   This enables compile-time checks (for example, `(set + 42)` is an error) and
   signal/arity tracking via `Binding` identity.

5. **File return value is the last expression.** If the last form is a `def`/`var`,
   the file returns the binding's name. If the last form is a bare expression,
   the file returns the expression's value. For empty files, the return value
   is `nil`. Modules return their last expression (typically a closure of exports).

6. **Macro expansion runs on the instance's `CompileCtx`.** Its macro VM and a
   clone of its `Expander`, which already holds the prelude, expand every form,
   so no compile parses the prelude again (docs/pipeline.md).
