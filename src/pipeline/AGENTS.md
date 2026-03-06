# pipeline

Compilation entry points: source text → bytecode or HIR.

## Responsibility

Orchestrate the full compilation pipeline:
- Reader: source text → Syntax
- Expander: Syntax → expanded Syntax (macro expansion)
- Analyzer: expanded Syntax → HIR (binding resolution, effect inference)
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
| `compile(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String>` | Compile a single expression to bytecode. Returns `CompileResult` with bytecode and warnings. |
| `compile_all(source: &str, symbols: &mut SymbolTable) -> Result<Vec<CompileResult>, String>` | Compile multiple top-level forms with fixpoint effect inference. Returns one `CompileResult` per form. **Deprecated in Chunk 1**: use `compile_file` instead. |
| `compile_file(source: &str, symbols: &mut SymbolTable) -> Result<CompileResult, String>` | Compile a file as a single synthetic letrec. All top-level forms are analyzed together, enabling mutual recursion. Returns a single `CompileResult`. |
| `analyze(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<AnalyzeResult, String>` | Analyze a single expression to HIR (no bytecode). Used by linter and LSP. |
| `analyze_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Vec<AnalyzeResult>, String>` | Analyze multiple top-level forms to HIR with fixpoint effect inference. Returns one `AnalyzeResult` per form. **Deprecated in Chunk 1**: use `analyze_file` instead. |
| `analyze_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<AnalyzeResult, String>` | Analyze a file as a single synthetic letrec (no bytecode). Used by linter and LSP for file-level analysis. |
| `eval(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | Compile and execute a single expression. Returns the result value. |
| `eval_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | Compile and execute multiple top-level forms sequentially. Returns the value of the last form. **Deprecated in Chunk 1**: use `eval_file` instead. |
| `eval_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>` | Compile and execute a file as a single synthetic letrec. Returns the value of the last expression. |
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
    ├─► (compile_file / analyze_file / eval_file)
    │   │
    │   ▼
    │   Analyzer.analyze_file_letrec (synthetic letrec)
    │   │
    │   ▼
    │   Analyzer.bind_primitives (wrap in primitive scope)
    │   │
    │   ▼
    │   HIR (single Letrec node)
    │
    └─► (compile_all / analyze_all / eval_all) [DEPRECATED]
        │
        ▼
        Fixpoint iteration (effect inference)
        │
        ▼
        Vec<HIR> (one per form)
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

## File-as-letrec model (Chunk 1)

The new model compiles a file to a **single compilation unit** instead of
a sequence of independent forms:

1. **Expand all forms** — macro expansion is idempotent
2. **Classify forms** — each form is `Def` (immutable), `Var` (mutable), or `Expr` (gensym-named)
3. **Analyze as letrec** — `Analyzer.analyze_file_letrec` does two-pass analysis:
   - Pass 1: pre-bind all names (enables mutual recursion)
   - Pass 2: analyze initializers sequentially
4. **Bind primitives** — `Analyzer.bind_primitives` wraps the letrec in an outer scope
   containing all registered primitives as immutable Global bindings
5. **Lower and emit** — standard LIR → Bytecode pipeline

**Key differences from `compile_all`:**
- Single `CompileResult` instead of `Vec<CompileResult>`
- All forms analyzed together (mutual recursion works)
- No fixpoint iteration needed (letrec pre-binding eliminates it)
- Primitives are lexical bindings, not just globals
- File's last expression is the return value

## Dependents

- `main.rs` — CLI file execution uses `eval_file`
- `primitives/import.rs` — module loading uses `compile_file`
- `lsp/state.rs` — file analysis uses `analyze_file`
- `lint/cli.rs` — linting uses `analyze_file`

## Invariants

1. **`compile` and `eval` are single-form entry points.** They parse a single
   expression, expand it, analyze it, and compile/execute it. No fixpoint
   iteration. Used for REPL and macro body evaluation.

2. **`compile_file`, `analyze_file`, `eval_file` are file-level entry points.**
   They parse all top-level forms, expand them, classify them, and analyze
   them as a single synthetic letrec via `Analyzer.analyze_file_letrec`.
   Used for file execution and module loading.

3. **`compile_all`, `analyze_all`, `eval_all` are deprecated.** They use
   fixpoint iteration and return `Vec<CompileResult>` / `Vec<AnalyzeResult>`.
   Callers should migrate to the file-as-letrec model.

4. **Primitives are pre-bound in file-level analysis.** `Analyzer.bind_primitives`
   wraps the file's letrec in an outer scope containing all registered primitives.
   This enables compile-time checks (e.g., `(set + 42)` is an error) and
   effect/arity tracking via `Binding` identity.

5. **File return value is the last expression.** If the last form is a `def`/`var`,
   the file returns the binding's name. If the last form is a bare expression,
   the file returns the expression's value. For empty files, the return value
   is `nil`.

6. **Macro expansion is cached.** The `cache` module maintains a thread-local
   `Expander` and `VM` for macro expansion. This avoids re-parsing the prelude
   and re-initializing the VM on every compilation.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~30 | Re-exports, `CompileResult`, `AnalyzeResult` |
| `compile.rs` | ~120 | `compile`, `compile_all` (deprecated), `compile_file` (new) |
| `analyze.rs` | ~65 | `analyze`, `analyze_all` (deprecated), `analyze_file` (new) |
| `eval.rs` | ~100 | `eval`, `eval_all` (deprecated), `eval_file` (new), `eval_syntax` |
| `cache.rs` | ~50 | Thread-local `Expander` and `VM` caching for macro expansion |
| `fixpoint.rs` | ~100 | Fixpoint iteration for effect/arity inference (used by `compile_all`) |
| `scan.rs` | ~80 | Pre-scanning forms for `def (name (fn ...))` patterns (used by `compile_all`) |
