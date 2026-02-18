# Elle Roadmap

> Last updated: February 2026

## Where we are

Elle has a single, clean compilation pipeline:

```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

The pipeline handles: lexical scoping with `BindingId`, closure capture
analysis with `LocalCell` for mutable captures, effect inference (`Pure`,
`Yields`, `Polymorphic`), tail call optimization, `handler-case` exception
handling, and coroutines with first-class continuations.

Source locations flow through the entire pipeline: Syntax spans → HIR spans →
LIR `SpannedInstr` → `LocationMap` in bytecode. Error messages include
file:line:col information.

Yield is a proper LIR terminator that splits functions into multiple basic
blocks. The emitter carries stack simulation state across yield boundaries.

### What works well

- Full compilation pipeline with property tests
- TCO (tail call optimization) — handles 50,000+ depth
- First-class continuations for coroutines across call boundaries
- Exception handlers preserved across yield/resume
- NaN-boxed 8-byte Value (Copy semantics)
- Source location tracking via LocationMap
- elle-lint and elle-lsp use the pipeline exclusively
- elle-doc generates the documentation site from Elle code
- Clean clippy, all tests pass

### What still needs work

- **8 ignored tests** — yield-from delegation (1), defmacro persistence
  (3), macro? primitive (1), expand-macro (1), module-qualified names (2).

## Completed phases

### Phase B: Hammer time (COMPLETED)

Removed dead code, migrated types, implemented source location tracking.

#### B.1: JIT deletion
- Deleted `src/compiler/cranelift/`, `compile/`, `converters/`, `ast.rs`,
  `jit_*.rs`, `primitives/jit.rs`, `effects/inference.rs`
- Removed `JitLambda`, `JitClosure`, `source_ast` from types
- Removed cranelift dependencies from Cargo.toml
- Removed `--jit` flag from main.rs
- ~12,500 lines removed, 4 crate dependencies removed

#### B.2: value_old migration
- Migrated all types to `value/` submodules
- `Closure` in `value/closure.rs` (with `location_map`)
- `Coroutine`, `CoroutineState` in `value/coroutine.rs`
- `Arity`, `SymbolId`, `NativeFn`, `VmAwareFn` in `value/types.rs`
- `LibHandle`, `CHandle` in `value/ffi.rs`
- Deleted `value_old/` module entirely

#### B.3: LocationMap implementation
- `SpannedInstr` wraps `LirInstr` with `Span`
- Lowerer propagates HIR spans to LIR
- Emitter builds `LocationMap` during emission
- `Closure` has `location_map: Rc<LocationMap>`
- VM uses per-closure location map in `capture_stack_trace`

#### B.4: Thread transfer tests
- Property tests for closure transfer with location data
- Integration tests for cross-thread error reporting

## What we're doing next

### Phase C: Macros and modules

Un-ignore the 7 macro/module tests by implementing:

- **defmacro persistence** — the Expander needs to persist across
  compilations within a session. Currently created fresh per `compile_new`
  call, so macros defined in one form aren't visible in the next.

- **macro? primitive** — requires runtime access to the macro registry.

- **expand-macro** — requires runtime macro expansion on quoted forms.

- **Module-qualified names** — `string/upcase`, `math/abs` syntax in the
  new pipeline. The HIR analyzer needs to resolve `module/name` references.

### Phase D: Documentation cleanup

Final cleanup pass:
- Update `docs/CPS_REWORK.md` to reflect completed state
- Audit file sizes (300-line target)
- Remove stale documentation (`docs/CPS_DESIGN.md`,
  `docs/LEXICAL_SCOPE_REFACTOR.md` — completed work)
- Update `refactor/` docs or remove if no longer needed

### Phase E: JIT (future)

Rewrite Cranelift JIT to consume LIR instead of Expr. This is a from-scratch
implementation using the preserved git history as reference. Prerequisites:
Phase C complete, LIR stable.

## Decisions made

| Decision | Rationale |
|----------|-----------|
| Delete JIT code, not feature-flag it | Git preserves history. Dead code has maintenance cost. |
| Full first-class continuations | More work than simple coroutine support, but composable and future-proof. |
| Yield as LIR terminator | Proper control flow modeling; prerequisite for future JIT. |
| Single execution path (bytecode) | CPS interpreter deleted. Simpler, fewer bugs, one thing to optimize. |
| `handler-case` not try/catch | Condition system is the exception mechanism. No Java-style try/catch. |
| Nil ≠ empty list | `nil` is falsy (absence), `()` is truthy (empty list). Lists terminate with `()`. |
| New pipeline skips Expr | Syntax → HIR directly. Expr was the old AST; no reason to generate it. |
| TCO via trampoline | `pending_tail_call` on VM, loop in `execute_bytecode`. Works for mutual recursion. |

## Known defects

- `handler-bind` is a stub (parsed, codegen ignores handlers)
- `InvokeRestart` opcode allocated but VM handler is no-op
- `signal`/`warn`/`error` are constructors, not signaling primitives
- yield-from delegation not implemented
- defmacro doesn't persist across compilation units
- Module-qualified names not supported
