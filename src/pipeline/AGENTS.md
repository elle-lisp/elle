# pipeline

Compilation entry points: source text → bytecode or HIR.

## Responsibility

Orchestrate the full compilation pipeline:
- Reader: source text → Syntax
- Expander: Syntax → expanded Syntax (macro expansion)
- Analyzer: expanded Syntax → HIR (binding resolution, signal inference)
- Lowerer: HIR → LIR (register allocation, basic blocks)
- Emitter: LIR → Bytecode (instruction encoding)
- VM: Bytecode → Value (execution)

Does NOT:
- Parse source (that's `reader`)
- Expand macros (that's `syntax`)
- Analyze bindings (that's `hir`)
- Generate code (that's `lir` and `compiler`)
- Execute bytecode (that's `vm`)

## Interface

| Function | Purpose |
|----------|---------|
| `compile(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String>` | Compile a single expression to bytecode. Returns `CompileResult` with bytecode. |
| `compile_file(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String>` | **PRIMARY FILE ENTRY POINT.** Compile a file as a single synthetic letrec. All top-level forms are analyzed together, enabling mutual recursion. Returns a single `CompileResult`. |
| `analyze(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<AnalyzeResult, String>` | Analyze a single expression to HIR (no bytecode). Used by linter and LSP. |
| `analyze_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<AnalyzeResult, String>` | **PRIMARY FILE ENTRY POINT.** Analyze a file as a single synthetic letrec (no bytecode). Used by linter and LSP for file-level analysis. |
| `eval(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | Compile and execute a single expression. Returns the result value. |
| `eval_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | **PRIMARY FILE ENTRY POINT.** Compile and execute a file as a single synthetic letrec. Returns the value of the last expression. |
| `eval_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | Convenience wrapper: compiles via `compile_file` (single letrec) then executes. Returns the value of the last form. Used by test helpers. |
| `eval_syntax(syntax: Syntax, expander: &mut Expander, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | Compile and execute a pre-parsed Syntax tree. Used internally by macro expansion. |

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

- `main.rs` — CLI file execution uses `eval_file`
- `primitives/modules.rs` — module loading uses `eval_file`
- `primitives/module_init.rs` — stdlib loading uses `compile_all` (internal)
- `lsp/state.rs` — file analysis uses `analyze_file`
- `lint/cli.rs` — linting uses `analyze_file`
- `tests/common/mod.rs` — test helpers use `eval_all`

## Invariants

1. **`compile` and `eval` are single-form entry points.** They parse a single
    expression, expand it, analyze it, and compile/execute it. Used for macro
    body evaluation. The REPL uses `compile_file` for multi-form support.

2. **`compile_file`, `analyze_file`, `eval_file` are file-level entry points.**
    They parse all top-level forms, expand them, classify them, and analyze
    them as a single synthetic letrec via `Analyzer.analyze_file_letrec`.
    Used for file execution, module loading, linting, and LSP.

3. **`eval_all` delegates to `compile_file`.** It compiles the source as a
    single letrec then executes it. Used by test helpers.

4. **`compile_all` is internal (`pub(crate)`).** Used only by `init_stdlib`
    to compile stdlib forms as independent globals. Not part of the public API.

5. **Primitives are pre-bound in file-level analysis.** `Analyzer.bind_primitives`
    wraps the file's letrec in an outer scope containing all registered primitives.
    This enables compile-time checks (e.g., `(set + 42)` is an error) and
    signal/arity tracking via `Binding` identity.

6. **File return value is the last expression.** If the last form is a `def`/`var`,
    the file returns the binding's name. If the last form is a bare expression,
    the file returns the expression's value. For empty files, the return value
    is `nil`. Modules return their last expression (typically a closure of exports).

7. **Macro expansion is cached.** The `cache` module maintains a thread-local
    `Expander` and `VM` for macro expansion. This avoids re-parsing the prelude
    and re-initializing the VM on every compilation.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~25 | Re-exports, `CompileResult`, `AnalyzeResult` |
| `compile.rs` | ~195 | `compile`, `compile_all` (internal), `compile_file`, `classify_form` |
| `analyze.rs` | ~75 | `analyze`, `analyze_file` |
| `eval.rs` | ~105 | `eval`, `eval_all`, `eval_file`, `eval_syntax` |
| `cache.rs` | ~50 | Thread-local `Expander` and `VM` caching for macro expansion |
