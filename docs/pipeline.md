# Compilation Pipeline

<!-- audited: 2026-10-06 -->

Compilation entry points: source reaches bytecode through the reader, expander, analyzer, lowerer and emitter.

Module: [src/pipeline](../src/pipeline/AGENTS.md).

## Contents

- [Public API](#public-api)
- [VM ownership patterns](#vm-ownership-patterns)
- [Expander lifecycle](#expander-lifecycle)
- [The fixpoint loop](#the-fixpoint-loop)
- [Compilation phases (single-form)](#compilation-phases-single-form)
- [Compile context](#compile-context)
- [Known issues](#known-issues)

| File | Purpose |
|------|---------|
| [mod.rs](../src/pipeline/mod.rs) | `CompileResult`, `AnalyzeResult`, re-exports |
| [cache.rs](../src/pipeline/cache.rs) | `CompileCtx`: per-instance compile state (macro VM, Expander, PrimitiveMeta, projection cache) |
| [bootstrap.rs](../src/pipeline/bootstrap.rs) | Compile and run core.lisp before any compile context exists |
| [sources.rs](../src/pipeline/sources.rs) | The core, prelude and stdlib sources a boot compiles, embedded at build time |
| [directives.rs](../src/pipeline/directives.rs) | Validate and strip `(elle/version …)` and `(elle/migration …)` |
| [compile.rs](../src/pipeline/compile.rs) | `compile()`, `compile_file()`, `compile_file_repl()`, and the whole-module entry points |
| [compile/frontend.rs](../src/pipeline/compile/frontend.rs) | Read, expand, and classify forms ahead of analysis |
| [compile/transforms.rs](../src/pipeline/compile/transforms.rs) | Post-analysis HIR transforms |
| [analyze.rs](../src/pipeline/analyze.rs) | `analyze()`, `analyze_file()` |
| [eval.rs](../src/pipeline/eval.rs) | `eval()`, `eval_all()`, `eval_syntax()`, `eval_file()` |

## Public API

### Types

```rust
pub struct CompileResult {
    pub bytecode: Bytecode,
}

pub struct AnalyzeResult {
    pub hir: Hir,
}
```

### Functions

| Function | VM for macros | Fixpoint? | Callers |
|----------|---------------|-----------|---------|
| `compile` | Internal | No | Integration tests |
| `compile_file` | Internal | Yes | `elle::program::run_source` (file, stdin, `-e`), `import-file`, the stdlib load |
| `eval` | Borrowed | No | Tests |
| `eval_all` | Internal (delegates to `compile_file`) | Yes | Tests |
| `eval_file` | Borrowed | Yes | Tests |
| `eval_syntax` | Borrowed | No | Macro body evaluation ([macro_expand.rs](../src/syntax/expand/macro_expand.rs)) |
| `analyze` | Borrowed | No | Tests |
| `analyze_file` | Borrowed | Yes | The LSP, the linter, `compile/analyze` |

### Signatures

The compile context (`cctx`) is the per-instance `CompileCtx` threaded
explicitly as a parameter through every entry point. `compile`/`compile_file`
expand macros on the context's own macro VM, so they need no caller VM; the
`eval`/`analyze` family run expansion on the caller's borrowed VM.

```rust
pub fn compile(source: &str, symbols: &mut SymbolTable, cctx: &mut CompileCtx, source_name: &str) -> Result<CompileResult, String>
pub fn compile_file(source: &str, symbols: &mut SymbolTable, cctx: &mut CompileCtx, source_name: &str) -> Result<CompileResult, String>
pub fn eval(source: &str, symbols: &mut SymbolTable, vm: &mut VM, cctx: &mut CompileCtx, source_name: &str) -> Result<Value, String>
pub fn eval_all(source: &str, symbols: &mut SymbolTable, vm: &mut VM, cctx: &mut CompileCtx, source_name: &str) -> Result<Value, String>
pub fn eval_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM, cctx: &mut CompileCtx, source_name: &str) -> Result<Value, String>
pub fn eval_syntax(syntax: Syntax, expander: &mut Expander, symbols: &mut SymbolTable, vm: &mut VM) -> Result<Value, String>
pub fn analyze(source: &str, symbols: &mut SymbolTable, vm: &mut VM, cctx: &mut CompileCtx, source_name: &str) -> Result<AnalyzeResult, String>
pub fn analyze_file(source: &str, symbols: &mut SymbolTable, vm: &mut VM, cctx: &mut CompileCtx, source_name: &str) -> Result<AnalyzeResult, String>
```

## VM ownership patterns

The macro-expansion VM is owned by the per-instance `CompileCtx` (built once
with primitives registered, core.lisp, and the prelude). Two distinct patterns
use it, and confusing them causes bugs:

**Context's macro VM** (`compile`, `compile_file`): These expand macros on the
`CompileCtx`'s own macro VM (`with_macro_expansion`, fiber reset between uses)
with a cloned `Expander`. They need no caller VM, so they take only
`symbols` + `cctx`. This is the correct pattern for batch compilation where
the caller doesn't need a running VM.

**Borrowed VM** (`eval`, `eval_syntax`, `analyze`, `analyze_file`):
These borrow the caller's `&mut VM`. The same VM is used for both macro
expansion and (for `eval`) execution, so macro side effects persist in the
caller's VM. This is the correct pattern for stdlib initialization and macro
body evaluation where state must accumulate. They obtain a cloned `Expander`
and `PrimitiveMeta` from the context via `cctx.expander_and_meta()`.

**Hybrid** (`eval_all`): Delegates compilation to `compile_file` (which expands
on the context's macro VM), then executes each compiled form on the caller's
borrowed VM. Macro side effects do NOT persist in the caller's VM.

**`eval_syntax` is special**: It also borrows the caller's `Expander`, not just
the VM. This is because it's called from inside `Expander::expand_macro_call_inner`
— the macro expansion engine needs to compile and run a macro body while
preserving the current expansion context (macro registry, scope state). Nested
macro calls work because the same Expander is threaded through. Its macro-body
metadata rides on the expander (`eval_meta`), so no separate `CompileCtx`
borrow is needed mid-expansion.

## Expander lifecycle

The prelude ([prelude.lisp](../src/prelude.lisp), embedded at build time) defines macros like
`defn`, `let*`, `when`, `unless` and `try`/`catch`. It is loaded once, into the
`CompileCtx`'s `Expander`, when the context is built ([cache.rs](../src/pipeline/cache.rs)). Every entry
point expands with a clone of that `Expander`: `compile` and `compile_file`
through `with_macro_expansion`, the `eval` and `analyze` families through
`expander_and_meta`. A clone carries the loaded prelude, so no call parses it
again.

`eval_syntax` reuses the caller's `Expander` because it runs mid-expansion.

## The fixpoint loop

Signal inference for mutually recursive definitions converges by fixpoint. The
loop lives in `analyze_file_letrec`
([fileletrec/letrec.rs](../src/hir/analyze/fileletrec/letrec.rs)),
not in the pipeline module: `compile_file` and `analyze_file` both classify a
file's forms and hand them to that one function, so the file *is* a letrec and
the file-level fixpoint and the letrec fixpoint are the same mechanism. Local
`(letrec ...)` forms run the same loop in
[letrec.rs](../src/hir/analyze/letrec.rs).

The signal inference computed here is exposed to tools and agents via:
- **`compile/signal`** — Get the inferred signal of a function
- **`portrait`** — Semantic portrait showing signal profile, composition properties, and observations
- **MCP server** — RDF knowledge graph with signal predicates (`elle:signal-yields`, `elle:signal-io`, etc.)

See [MCP server documentation](mcp.md) and [Agent Reasoning](analysis/agent-reasoning.md) for how to query this information.

### Problem

In `(def f (fn (x) (g x)))` followed by `(def g (fn (x) (yield x)))`, a single
sequential pass analyzes `f` while `g` is still unanalyzed. `f` therefore reads
whatever `g`'s seed says rather than what `g` turns out to be, and a second
name in the cycle can make the first one's answer wrong in either direction:
too low if the seed is optimistic, too high if it is conservative.

### Algorithm

1. **Pre-bind** every name in the letrec, so each initializer can resolve its
   siblings. Seed each one's signal with `Signal::silent()`.

2. **Analyze initializers** in order. Record each lambda's `inferred_signals`
   in `signal_env` and its arity in `arity_env` as it is analyzed. Bindings
   analyzed early in this pass may have read stale sibling signals; the next
   step is what repairs that.

3. **Iterate to a fixpoint** (bounded at 10 iterations). Re-analyze every
   lambda binding against the current `signal_env`. If any lambda's
   `inferred_signals` differs from what `signal_env` holds, record the new
   value and iterate again. Stop on the first iteration that changes nothing.

4. **Analyze the body** (or, for a file-letrec, the remaining expression
   entries) only after the loop has converged, so calls into the cycle read
   settled signals.

5. **Combine** the aggregate signal from the post-fixpoint binding and body
   HIR, not from the values step 2 produced.

Re-analysis is safe to repeat: its side effects on bindings (`mark_captured`,
`mark_mutated`) only ever add flags, so an extra iteration can make the result
more conservative but never wrong. Errors raised inside lambda bodies are
collected from the final iteration only, so a body that fails to analyze does
not report the same error once per iteration.

### Convergence

Signals form a lattice and the seed is the bottom element, so each iteration
can only move a signal upward. A signal that stops moving is at its least
fixed point. Termination is therefore guaranteed by the lattice height; the
10-iteration bound is a safety net against a lattice bug, not part of the
argument. In practice convergence takes 1–3 iterations.

Because the seed is optimistic, a loop that stops early reports signals that
are too *low* — it under-approximates. That is the dangerous direction: a
function that can raise `:error` but is inferred silent will pass a
compile-time `(silence)` check and fail at runtime instead. The runtime
enforcement in `attune`/`silence` is the backstop, not the guarantee.

### Scope

Convergence is per-file. Mutual recursion across a file boundary does not
converge, because each import is a separate compilation — see
[signals/inference.md](signals/inference.md) for why that is a design choice and what `squelch` does about it.


## Compilation phases (single-form)

Every compilation path follows the same phases:

1. **Read**: `read_syntax(arena, source, source_name)` → `Syntax`
2. **Expand**: `expander.expand(syntax, symbols, macro_vm)` → expanded `Syntax`
3. **Analyze**: `Analyzer::new_with_primitives(...)` →
   `analyzer.analyze(&expanded)` → `AnalysisResult { hir, .. }`
4. **Regularize**: `crate::hir::regularize(&mut hir, ...)` marks tail calls,
   functionalizes, lifts to ANF and infers types, in place
5. **Regions**: `analyze_regions_with(&hir, ...)` → `RegionInfo`
6. **Lower**: `Lowerer::new(&arena)...lower(&hir)` → `LirModule`
7. **Freeze**: `LirModule::freeze()` → `FrozenModule`
   ([lir.md](impl/lir.md) § "Two forms of one function")
8. **Emit**: `Emitter::new().emit_module(&frozen)` → `Bytecode`

`analyze` and `analyze_file` stop after phase 3, then mark tail calls alone
(no regularization, lowering or emission).

## Compile context

[cache.rs](../src/pipeline/cache.rs) holds it. `CompileCtx` is the per-instance compile-time state: a macro-expansion VM
(primitives registered), the core.lisp/prelude `Expander`, the
`PrimitiveMeta`, and the file→signal projection cache. It is built once when
the instance's `RuntimeCore` is constructed and threaded explicitly through
every pipeline call — two embedded Elle instances on one thread each own their
own, so neither sees the other's exports or REPL definitions.

### `with_macro_expansion()`

Runs a closure with the macro-expansion VM (fiber reset), a clone of the
`Expander` (independent expansion state), and a clone of the compile `meta`.
The clones decouple it from `self`'s borrow so a nested compile does not alias.
Used by `compile` and `compile_file`.

### `expander_and_meta()`

Returns a cloned `(Expander, PrimitiveMeta)` without borrowing the macro VM.
Used by `eval`, `analyze`, and `analyze_file`, which run expansion on their
own VM.

### Invariants

- Prelude must be 100% defmacro (no runtime definitions)
- Primitives must be registered in the context's macro VM at construction
- Pipeline functions are not re-entrant (no nested compile/compile_file)

## Known issues

Single-form functions
(`compile`, `eval`, `analyze`) don't benefit from cross-form signal inference —
a file compiled via `compile` instead of `compile_file` will treat all
cross-form calls as `Polymorphic`. The REPL compiles each form individually
via `compile_file_repl` and registers def bindings in the compilation cache
(`register_repl_binding`) so they are visible to subsequent compilations.
However, cross-form signal inference within a single REPL input is limited
to what `compile_file` can infer for each form in isolation.
