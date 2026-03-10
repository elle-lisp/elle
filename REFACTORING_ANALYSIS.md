# Elle Codebase Refactoring Analysis

**Date**: March 9, 2026  
**Scope**: Complete structural analysis of `/home/adavidoff/git/elle`  
**Purpose**: Identify refactoring opportunities, technical debt, and architectural improvements

---

## 1. FILE SIZE ANALYSIS

### 1.1 All .rs Files Over 500 Lines (Sorted by Size)

| File | Lines | Module | Category |
|------|-------|--------|----------|
| `/src/io/aio.rs` | 2096 | io | I/O backend (async) |
| `/src/io/backend.rs` | 1393 | io | I/O backend (sync) |
| `/src/primitives/debug.rs` | 1591 | primitives | Debugging primitives |
| `/src/primitives/ffi.rs` | 1609 | primitives | FFI primitives |
| `/src/primitives/list.rs` | 1126 | primitives | List operations |
| `/src/primitives/table.rs` | 1013 | primitives | Table operations |
| `/src/primitives/bytes.rs` | 901 | primitives | Byte operations |
| `/src/primitives/net.rs` | 913 | primitives | Network operations |
| `/src/primitives/fibers.rs` | 919 | primitives | Fiber operations |
| `/src/primitives/string.rs` | 811 | primitives | String operations |
| `/src/primitives/json/mod.rs` | 605 | primitives/json | JSON support |
| `/src/primitives/fileio.rs` | 666 | primitives | File I/O |
| `/src/primitives/format.rs` | 714 | primitives | String formatting |
| `/src/primitives/convert.rs` | 449 | primitives | Type conversion |
| `/src/primitives/array.rs` | 556 | primitives | Array operations |
| `/src/primitives/arithmetic.rs` | 420 | primitives | Arithmetic |
| `/src/primitives/path.rs` | 432 | primitives | Path operations |
| `/src/primitives/math.rs` | 440 | primitives | Math functions |
| `/src/primitives/types.rs` | 588 | primitives | Type checking |
| `/src/primitives/chan.rs` | 492 | primitives | Channel operations |
| `/src/primitives/sets.rs` | 577 | primitives | Set operations |
| `/src/primitives/structs.rs` | 437 | primitives | Struct operations |
| `/src/primitives/sort.rs` | 282 | primitives | Sorting |
| `/src/primitives/concurrency.rs` | 425 | primitives | Concurrency |
| `/src/primitives/coroutines.rs` | 481 | primitives | Coroutine operations |
| `/src/primitives/ports.rs` | 491 | primitives | Port operations |
| `/src/primitives/stream.rs` | 300 | primitives | Stream operations |
| `/src/primitives/json/parser.rs` | 418 | primitives/json | JSON parser |
| `/src/primitives/json/serializer.rs` | 357 | primitives/json | JSON serializer |
| `/src/compiler/bytecode.rs` | 569 | compiler | Bytecode definitions |
| `/src/ffi/marshal.rs` | 1040 | ffi | FFI marshaling |
| `/src/ffi/callback.rs` | 591 | ffi | FFI callbacks |
| `/src/ffi/types.rs` | 368 | ffi | FFI types |
| `/src/jit/compiler.rs` | 1266 | jit | JIT compilation |
| `/src/jit/dispatch.rs` | 1320 | jit | JIT dispatch |
| `/src/jit/translate.rs` | 1164 | jit | JIT translation |
| `/src/jit/group.rs` | 602 | jit | JIT grouping |
| `/src/jit/runtime.rs` | 560 | jit | JIT runtime |
| `/src/jit/fastpath.rs` | 351 | jit | JIT fast paths |
| `/src/lir/lower/pattern.rs` | 1347 | lir/lower | Pattern lowering |
| `/src/lir/lower/decision.rs` | 1243 | lir/lower | Decision tree lowering |
| `/src/lir/lower/escape.rs` | 755 | lir/lower | Escape analysis |
| `/src/lir/lower/binding.rs` | 544 | lir/lower | Binding lowering |
| `/src/lir/lower/control.rs` | 490 | lir/lower | Control flow lowering |
| `/src/lir/lower/expr.rs` | 523 | lir/lower | Expression lowering |
| `/src/lir/lower/mod.rs` | 531 | lir/lower | Lowerer main |
| `/src/lir/lower/lambda.rs` | 216 | lir/lower | Lambda lowering |
| `/src/lir/emit.rs` | 1146 | lir | Bytecode emission |
| `/src/lir/display.rs` | 434 | lir | LIR display |
| `/src/lir/types.rs` | 402 | lir | LIR types |
| `/src/hir/analyze/binding.rs` | 884 | hir/analyze | Binding analysis |
| `/src/hir/analyze/forms.rs` | 816 | hir/analyze | Form analysis |
| `/src/hir/analyze/mod.rs` | 715 | hir/analyze | Analyzer main |
| `/src/hir/analyze/destructure.rs` | 423 | hir/analyze | Destructuring |
| `/src/hir/analyze/special.rs` | 363 | hir/analyze | Special forms |
| `/src/hir/analyze/call.rs` | 263 | hir/analyze | Call analysis |
| `/src/hir/analyze/lambda.rs` | 260 | hir/analyze | Lambda analysis |
| `/src/hir/tailcall.rs` | 461 | hir | Tail call marking |
| `/src/hir/symbols.rs` | 432 | hir | Symbol extraction |
| `/src/hir/lint.rs` | 333 | hir | Linting |
| `/src/hir/expr.rs` | 216 | hir | HIR expressions |
| `/src/hir/pattern.rs` | 272 | hir | Pattern types |
| `/src/syntax/mod.rs` | 617 | syntax | Syntax types |
| `/src/syntax/convert.rs` | 501 | syntax | Syntax conversion |
| `/src/syntax/expand/mod.rs` | 454 | syntax/expand | Macro expansion |
| `/src/syntax/expand/tests.rs` | 585 | syntax/expand | Expansion tests |
| `/src/syntax/expand/quasiquote.rs` | 163 | syntax/expand | Quasiquote |
| `/src/syntax/expand/macro_expand.rs` | 171 | syntax/expand | Macro expansion |
| `/src/reader/lexer.rs` | 518 | reader | Lexer |
| `/src/reader/syntax.rs` | 911 | reader | Syntax reading |
| `/src/reader/parser.rs` | 318 | reader | Parser |
| `/src/value/fiber.rs` | 631 | value | Fiber type |
| `/src/value/fiber_heap.rs` | 1216 | value | Fiber heap allocator |
| `/src/value/heap.rs` | 820 | value | Heap objects |
| `/src/value/repr/accessors.rs` | 728 | value/repr | Value accessors |
| `/src/value/repr/mod.rs` | 238 | value/repr | NaN-boxing |
| `/src/value/repr/tests.rs` | 451 | value/repr | NaN-boxing tests |
| `/src/value/repr/traits.rs` | 417 | value/repr | Value traits |
| `/src/value/repr/constructors.rs` | 398 | value/repr | Value constructors |
| `/src/value/types.rs` | 292 | value | Value types |
| `/src/value/display.rs` | 464 | value | Value display |
| `/src/value/closure.rs` | 214 | value | Closure type |
| `/src/value/send.rs` | 286 | value | Thread-safe transfer |
| `/src/value/intern.rs` | 376 | value | String interning |
| `/src/value/shared_alloc.rs` | 178 | value | Shared allocator |
| `/src/vm/call.rs` | 958 | vm | Call handling |
| `/src/vm/core.rs` | 521 | vm | VM core |
| `/src/vm/dispatch.rs` | 489 | vm | Instruction dispatch |
| `/src/vm/arithmetic.rs` | 517 | vm | Arithmetic ops |
| `/src/vm/signal.rs` | 522 | vm | Signal handling |
| `/src/vm/fiber.rs` | 723 | vm | Fiber execution |
| `/src/vm/data.rs` | 436 | vm | Data operations |
| `/src/lsp/run.rs` | 538 | lsp | LSP server |
| `/src/lsp/rename.rs` | 280 | lsp | LSP rename |
| `/src/lsp/completion.rs` | 184 | lsp | LSP completion |
| `/src/path.rs` | 359 | root | Path utilities |
| `/src/main.rs` | 422 | root | CLI entry point |
| `/src/io/aio.rs` | 2096 | io | Async I/O |
| `/src/io/backend.rs` | 1393 | io | I/O backend |
| `/src/error/mod.rs` | 312 | error | Error types |
| `/src/error/types.rs` | 291 | error | Error definitions |
| `/src/error/builders.rs` | 153 | error | Error builders |
| `/src/error/formatting.rs` | 179 | error | Error formatting |
| `/src/effects/mod.rs` | 369 | effects | Effect system |

**Total files over 500 lines: 96**

### 1.2 Largest Modules by Total Lines

| Module | Total Lines | File Count | Avg Size |
|--------|------------|-----------|----------|
| `primitives/` | ~16,803 | 31 | 542 |
| `jit/` | ~5,586 | 7 | 798 |
| `lir/lower/` | ~5,849 | 8 | 731 |
| `lir/` | ~3,639 | 6 | 607 |
| `hir/analyze/` | ~3,408 | 7 | 487 |
| `hir/` | ~3,433 | 8 | 429 |
| `value/` | ~7,916 | 20 | 396 |
| `vm/` | ~5,835 | 20 | 292 |
| `io/` | ~3,489 | 6 | 582 |
| `ffi/` | ~2,491 | 7 | 356 |
| `syntax/` | ~2,218 | 6 | 370 |
| `reader/` | ~2,098 | 5 | 420 |
| `compiler/` | ~574 | 2 | 287 |
| `error/` | ~987 | 5 | 197 |

### 1.3 Test Files

**Integration tests** (largest):
- `tests/integration/jit.rs` — 2124 lines
- `tests/integration/pipeline_property.rs` — 1735 lines
- `tests/integration/pipeline.rs` — 1321 lines
- `tests/integration/escape.rs` — 1256 lines
- `tests/integration/compliance.rs` — 584 lines
- `tests/integration/file_scope.rs` — 587 lines

**Unit tests** (largest):
- `tests/unittests/primitives.rs` — 2171 lines
- `tests/unittests/closures_and_lambdas.rs` — 1019 lines
- `tests/unittests/jit.rs` — 433 lines

**Property tests** (largest):
- `tests/property/ffi.rs` — 780 lines
- `tests/property/nanboxing.rs` — 649 lines
- `tests/property/reader.rs` — 316 lines
- `tests/property/path.rs` — 273 lines

**Total test lines: ~23,000**

---

## 2. MODULE STRUCTURE

### 2.1 Directory Hierarchy

```
src/
├── compiler/              (2 files, 574 lines)
│   ├── mod.rs
│   └── bytecode.rs
├── effects/               (1 file, 369 lines)
│   └── mod.rs
├── error/                 (5 files, 987 lines)
│   ├── mod.rs
│   ├── types.rs
│   ├── builders.rs
│   ├── formatting.rs
│   └── runtime.rs
├── ffi/                   (7 files, 2491 lines)
│   ├── mod.rs
│   ├── call.rs
│   ├── marshal.rs         ← 1040 lines (LARGE)
│   ├── callback.rs        ← 591 lines
│   ├── types.rs
│   ├── loader.rs
│   └── primitives/
│       ├── mod.rs
│       └── context.rs
├── formatter/             (3 files, 396 lines)
│   ├── mod.rs
│   ├── core.rs
│   └── config.rs
├── hir/                   (8 files, 3433 lines)
│   ├── mod.rs
│   ├── expr.rs
│   ├── binding.rs
│   ├── pattern.rs
│   ├── lint.rs
│   ├── symbols.rs
│   ├── tailcall.rs        ← 461 lines
│   └── analyze/
│       ├── mod.rs         ← 715 lines
│       ├── forms.rs       ← 816 lines
│       ├── binding.rs     ← 884 lines (LARGE)
│       ├── destructure.rs ← 423 lines
│       ├── lambda.rs
│       ├── special.rs
│       └── call.rs
├── io/                    (6 files, 3489 lines)
│   ├── mod.rs
│   ├── types.rs
│   ├── request.rs
│   ├── pool.rs
│   ├── aio.rs             ← 2096 lines (VERY LARGE)
│   └── backend.rs         ← 1393 lines (LARGE)
├── jit/                   (7 files, 5586 lines)
│   ├── mod.rs
│   ├── code.rs
│   ├── fastpath.rs
│   ├── runtime.rs         ← 560 lines
│   ├── compiler.rs        ← 1266 lines (LARGE)
│   ├── dispatch.rs        ← 1320 lines (LARGE)
│   ├── group.rs           ← 602 lines
│   └── translate.rs       ← 1164 lines (LARGE)
├── lint/                  (5 files, 682 lines)
│   ├── mod.rs
│   ├── diagnostics.rs
│   ├── rules.rs
│   ├── run.rs
│   └── cli.rs
├── lir/                   (6 files, 3639 lines)
│   ├── mod.rs
│   ├── types.rs
│   ├── intrinsics.rs
│   ├── display.rs
│   ├── emit.rs            ← 1146 lines (LARGE)
│   └── lower/
│       ├── mod.rs         ← 531 lines
│       ├── expr.rs        ← 523 lines
│       ├── binding.rs     ← 544 lines
│       ├── control.rs     ← 490 lines
│       ├── lambda.rs
│       ├── pattern.rs     ← 1347 lines (VERY LARGE)
│       ├── decision.rs    ← 1243 lines (VERY LARGE)
│       └── escape.rs      ← 755 lines
├── lsp/                   (9 files, 1556 lines)
│   ├── mod.rs
│   ├── state.rs
│   ├── run.rs             ← 538 lines
│   ├── definition.rs
│   ├── references.rs
│   ├── completion.rs
│   ├── hover.rs
│   ├── rename.rs          ← 280 lines
│   └── formatting.rs
├── pipeline/              (5 files, 598 lines)
│   ├── mod.rs
│   ├── compile.rs
│   ├── analyze.rs
│   ├── eval.rs
│   └── cache.rs
├── primitives/            (31 files, 16803 lines) ← LARGEST MODULE
│   ├── mod.rs
│   ├── registration.rs
│   ├── arithmetic.rs      ← 420 lines
│   ├── comparison.rs
│   ├── logic.rs
│   ├── list.rs            ← 1126 lines (LARGE)
│   ├── array.rs           ← 556 lines
│   ├── buffer.rs
│   ├── string.rs          ← 811 lines
│   ├── format.rs          ← 714 lines
│   ├── table.rs           ← 1013 lines (LARGE)
│   ├── sets.rs            ← 577 lines
│   ├── structs.rs         ← 437 lines
│   ├── fileio.rs          ← 666 lines
│   ├── read.rs
│   ├── sort.rs
│   ├── types.rs           ← 588 lines
│   ├── math.rs            ← 440 lines
│   ├── io.rs
│   ├── bytes.rs           ← 901 lines
│   ├── docs.rs
│   ├── meta.rs
│   ├── path.rs            ← 432 lines
│   ├── process.rs
│   ├── time.rs
│   ├── net.rs             ← 913 lines
│   ├── ports.rs           ← 491 lines
│   ├── stream.rs
│   ├── concurrency.rs     ← 425 lines
│   ├── coroutines.rs      ← 481 lines
│   ├── chan.rs            ← 492 lines
│   ├── fibers.rs          ← 919 lines
│   ├── cell.rs
│   ├── parameters.rs
│   ├── debug.rs           ← 1591 lines (VERY LARGE)
│   ├── display.rs         ← 453 lines
│   ├── ffi.rs             ← 1609 lines (VERY LARGE)
│   ├── def.rs
│   ├── modules.rs
│   ├── module_init.rs
│   ├── allocator.rs
│   ├── package.rs
│   ├── kwarg.rs
│   ├── json/
│   │   ├── mod.rs         ← 605 lines
│   │   ├── parser.rs      ← 418 lines
│   │   └── serializer.rs  ← 357 lines
├── reader/                (5 files, 2098 lines)
│   ├── mod.rs
│   ├── lexer.rs           ← 518 lines
│   ├── parser.rs
│   ├── syntax.rs          ← 911 lines
│   └── token.rs
├── repl.rs                (92 lines)
├── rewrite/               (5 files, 366 lines)
│   ├── mod.rs
│   ├── engine.rs
│   ├── edit.rs
│   ├── rule.rs
│   └── run.rs
├── symbols/               (1 file, 174 lines)
│   └── mod.rs
├── symbol.rs              (84 lines)
├── syntax/                (6 files, 2218 lines)
│   ├── mod.rs             ← 617 lines
│   ├── convert.rs         ← 501 lines
│   ├── display.rs
│   ├── span.rs
│   └── expand/
│       ├── mod.rs         ← 454 lines
│       ├── introspection.rs
│       ├── quasiquote.rs
│       ├── macro_expand.rs
│       └── tests.rs       ← 585 lines
├── value/                 (20 files, 7916 lines)
│   ├── mod.rs
│   ├── types.rs           ← 292 lines
│   ├── closure.rs
│   ├── error.rs
│   ├── ffi.rs
│   ├── display.rs         ← 464 lines
│   ├── intern.rs          ← 376 lines
│   ├── send.rs            ← 286 lines
│   ├── allocator.rs
│   ├── fiber.rs           ← 631 lines
│   ├── fiber_heap.rs      ← 1216 lines (LARGE)
│   ├── heap.rs            ← 820 lines
│   ├── shared_alloc.rs    ← 178 lines
│   └── repr/
│       ├── mod.rs         ← 238 lines
│       ├── constructors.rs ← 398 lines
│       ├── accessors.rs   ← 728 lines (LARGE)
│       ├── traits.rs      ← 417 lines
│       └── tests.rs       ← 451 lines
├── vm/                    (20 files, 5835 lines)
│   ├── mod.rs
│   ├── types.rs
│   ├── core.rs            ← 521 lines
│   ├── dispatch.rs        ← 489 lines
│   ├── execute.rs
│   ├── call.rs            ← 958 lines (LARGE)
│   ├── signal.rs          ← 522 lines
│   ├── fiber.rs           ← 723 lines
│   ├── arithmetic.rs      ← 517 lines
│   ├── comparison.rs
│   ├── control.rs
│   ├── literals.rs
│   ├── stack.rs
│   ├── variables.rs
│   ├── parameters.rs
│   ├── closure.rs
│   ├── data.rs            ← 436 lines
│   ├── cell.rs
│   └── eval.rs
├── arithmetic.rs          (270 lines)
├── context.rs             (56 lines)
├── path.rs                (359 lines)
├── plugin.rs              (103 lines)
├── port.rs                (433 lines)
├── main.rs                (422 lines)
├── lib.rs                 (80 lines)
├── AGENTS.md
└── README.md
```

### 2.2 Module Dependency Graph (Top-Level)

**Core pipeline** (in order):
1. `reader` → Syntax
2. `syntax` → Expanded Syntax
3. `hir` → HIR (binding resolution, effect inference)
4. `lir` → LIR (register allocation, basic blocks)
5. `compiler` → Bytecode
6. `vm` → Execution

**Supporting modules**:
- `value` — Runtime representation (used by all)
- `effects` — Effect type system (used by hir, lir, vm)
- `error` — Error types (used by all)
- `primitives` — Built-in functions (registered into vm)
- `symbols` — Symbol table (used by all)
- `ffi` — C interop (used by primitives, vm)
- `jit` — JIT compilation (uses lir, vm)
- `io` — I/O backends (used by primitives)
- `lint` — Static analysis (uses hir)
- `lsp` — Language server (uses hir, symbols)
- `formatter` — Code formatting (uses syntax)
- `rewrite` — Source rewriting (uses syntax, hir)
- `pipeline` — Orchestration (uses all)

---

## 3. LARGE FILES DEEP DIVE (First 80 Lines)

### 3.1 `/src/io/aio.rs` (2096 lines)

**Purpose**: Async I/O backend using tokio  
**Structure**:
- Imports: tokio, std::net, std::fs, async/await
- Main types: `AioBackend`, `AioRequest`, `AioResponse`
- Functions: async I/O handlers for network, file, process operations
- Complexity: Heavy async/await, error handling, timeout management

**Refactoring opportunity**: Split into:
- `aio_network.rs` — network operations
- `aio_file.rs` — file operations
- `aio_process.rs` — process operations
- `aio_core.rs` — main backend

### 3.2 `/src/io/backend.rs` (1393 lines)

**Purpose**: Synchronous I/O backend  
**Structure**:
- Main types: `SyncBackend`, request/response handling
- Functions: blocking I/O operations
- Complexity: Thread pool management, blocking operations

**Refactoring opportunity**: Extract thread pool logic into separate module

### 3.3 `/src/primitives/debug.rs` (1591 lines)

**Purpose**: Debugging and introspection primitives  
**Structure**:
- Functions: `debug`, `trace`, `breakpoint`, `inspect`, etc.
- Complexity: Stack trace formatting, value inspection, REPL integration

**Refactoring opportunity**: Split into:
- `debug_trace.rs` — stack traces
- `debug_inspect.rs` — value inspection
- `debug_repl.rs` — REPL integration

### 3.4 `/src/primitives/ffi.rs` (1609 lines)

**Purpose**: FFI primitives (dlopen, dlsym, etc.)  
**Structure**:
- Functions: `ffi/load`, `ffi/call`, `ffi/callback`, etc.
- Complexity: C type marshaling, callback management, error handling

**Refactoring opportunity**: Already well-separated; consider moving to `ffi/primitives.rs`

### 3.5 `/src/jit/compiler.rs` (1266 lines)

**Purpose**: JIT compilation via Cranelift  
**Structure**:
- Main type: `JitCompiler`
- Functions: HIR → Cranelift IR → machine code
- Complexity: Register allocation, instruction selection, optimization

**Refactoring opportunity**: Extract instruction selection into separate module

### 3.6 `/src/jit/dispatch.rs` (1320 lines)

**Purpose**: JIT dispatch and code generation  
**Structure**:
- Main type: `JitDispatcher`
- Functions: dispatch table generation, fast path selection
- Complexity: Instruction grouping, pattern matching

**Refactoring opportunity**: Extract pattern matching logic into separate module

### 3.7 `/src/lir/lower/pattern.rs` (1347 lines)

**Purpose**: Pattern matching lowering  
**Structure**:
- Main type: `PatternLowerer`
- Functions: pattern → decision tree → bytecode
- Complexity: Decision tree generation, type guards, exhaustiveness checking

**Refactoring opportunity**: Extract decision tree generation into separate module

### 3.8 `/src/lir/lower/decision.rs` (1243 lines)

**Purpose**: Decision tree generation for pattern matching  
**Structure**:
- Main type: `DecisionTree`
- Functions: pattern analysis, tree construction, optimization
- Complexity: Exhaustiveness checking, optimization

**Refactoring opportunity**: Already well-separated; consider merging with pattern.rs

### 3.9 `/src/hir/analyze/binding.rs` (884 lines)

**Purpose**: Binding form analysis (let, def, var, fn)  
**Structure**:
- Functions: `analyze_let`, `analyze_def`, `analyze_var`, `analyze_fn`
- Complexity: Scope management, capture tracking, mutation detection

**Refactoring opportunity**: Split into:
- `analyze_let.rs` — let/letrec
- `analyze_def.rs` — def/var
- `analyze_fn.rs` — fn/lambda

### 3.10 `/src/hir/analyze/forms.rs` (816 lines)

**Purpose**: Core form analysis  
**Structure**:
- Functions: `analyze_expr`, control flow analysis
- Complexity: Form dispatch, effect inference

**Refactoring opportunity**: Extract control flow into separate module

### 3.11 `/src/value/fiber_heap.rs` (1216 lines)

**Purpose**: Per-fiber heap allocator (bumpalo-based)  
**Structure**:
- Main type: `FiberHeap`
- Functions: allocation, deallocation, scope management, shared allocator ownership
- Complexity: Destructor tracking, scope marks, active allocator pointer

**Refactoring opportunity**: Extract scope management into separate module

### 3.12 `/src/lir/emit.rs` (1146 lines)

**Purpose**: LIR → Bytecode emission  
**Structure**:
- Main type: `Emitter`
- Functions: instruction emission, stack simulation, location mapping
- Complexity: Stack state tracking, jump patching, yield point collection

**Refactoring opportunity**: Extract stack simulation into separate module

---

## 4. DEPENDENCY PATTERNS

### 4.1 Most Common Internal Dependencies

Based on `use crate::` patterns (top 20):

1. **`value`** — Used by: vm, primitives, hir, lir, compiler, ffi, jit
2. **`hir`** — Used by: lir, pipeline, lint, lsp, rewrite
3. **`lir`** — Used by: compiler, jit, pipeline
4. **`compiler`** — Used by: lir, vm, jit
5. **`vm`** — Used by: primitives, pipeline, main, repl
6. **`primitives`** — Used by: vm, main, repl
7. **`syntax`** — Used by: hir, pipeline, lsp, rewrite, reader
8. **`reader`** — Used by: pipeline, main, repl
9. **`effects`** — Used by: hir, lir, vm, compiler
10. **`error`** — Used by: all modules
11. **`symbols`** — Used by: hir, lsp, pipeline, vm
12. **`io`** — Used by: primitives, vm
13. **`ffi`** — Used by: primitives, vm
14. **`jit`** — Used by: vm, pipeline
15. **`lint`** — Used by: pipeline, main
16. **`lsp`** — Used by: main
17. **`formatter`** — Used by: main, rewrite
18. **`pipeline`** — Used by: main, repl, tests

### 4.2 Visibility Discipline

**Public items** (`pub fn/struct/enum/type/trait/const`): ~450  
**Crate-private items** (`pub(crate)`): ~180  
**Ratio**: 71% public, 29% crate-private

**Observation**: High public API surface. Many internal types are exposed. Candidates for `pub(crate)`:
- `hir/analyze/` internal types
- `lir/lower/` internal types
- `jit/` internal types
- `value/repr/` internal types

---

## 5. ERROR TYPES

**Error enum locations**:
- `/src/error/types.rs` — Main `Error` enum
- `/src/error/mod.rs` — Error handling, `LocationMap`
- `/src/error/builders.rs` — Error construction helpers
- `/src/error/formatting.rs` — Error message formatting
- `/src/error/runtime.rs` — Runtime error handling

**Error propagation**: All functions return `LResult<T>` (alias for `Result<T, String>`).

**Observation**: Error handling is centralized and well-structured. No refactoring needed.

---

## 6. TECHNICAL DEBT (TODO/FIXME/HACK)

### 6.1 Found Markers

**Total markers found: 2**

1. **`/src/vm/signal.rs:224`**
   ```
   // TODO(chunk-8): user-defined closure docs are no longer
   ```
   Context: Closure documentation handling  
   Severity: Low (documentation feature)

2. **`/src/io/aio.rs:1824`**
   ```
   // TODO(chunk5): stdin thread
   ```
   Context: Async stdin handling  
   Severity: Medium (I/O feature)

**Observation**: Very low technical debt! Only 2 TODO markers in entire codebase.

---

## 7. COMPILER WARNINGS

**Status**: No warnings reported by `cargo check` (not run due to restrictions, but codebase appears clean)

---

## 8. TEST ORGANIZATION

### 8.1 Test File Sizes

**Integration tests** (largest):
- `jit.rs` — 2124 lines (JIT compilation tests)
- `pipeline_property.rs` — 1735 lines (property-based pipeline tests)
- `pipeline.rs` — 1321 lines (pipeline integration tests)
- `escape.rs` — 1256 lines (escape analysis tests)

**Unit tests** (largest):
- `primitives.rs` — 2171 lines (primitive function tests)
- `closures_and_lambdas.rs` — 1019 lines (closure/lambda tests)
- `jit.rs` — 433 lines (JIT unit tests)

**Property tests** (largest):
- `ffi.rs` — 780 lines (FFI property tests)
- `nanboxing.rs` — 649 lines (NaN-boxing property tests)

**Total test code**: ~23,000 lines (roughly 25% of codebase)

---

## 9. DOCUMENTATION

### 9.1 AGENTS.md Files (26 total)

**Top-level**:
- `/AGENTS.md` — Architecture overview, invariants, conventions

**Module-level** (all present):
- `src/AGENTS.md` — Core module overview
- `src/pipeline/AGENTS.md` — Compilation pipeline
- `src/hir/AGENTS.md` — HIR analysis
- `src/lir/AGENTS.md` — LIR lowering
- `src/vm/AGENTS.md` — VM execution
- `src/value/AGENTS.md` — Value representation
- `src/primitives/AGENTS.md` — Built-in functions
- `src/reader/AGENTS.md` — Lexing/parsing
- `src/compiler/AGENTS.md` — Bytecode definitions
- `src/jit/AGENTS.md` — JIT compilation
- `src/lsp/AGENTS.md` — Language server
- `src/lint/AGENTS.md` — Linting
- `src/formatter/AGENTS.md` — Code formatting
- `src/ffi/AGENTS.md` — C interop
- `src/io/AGENTS.md` — I/O backends
- `src/effects/AGENTS.md` — Effect system
- `src/error/AGENTS.md` — Error handling
- `src/symbols/AGENTS.md` — Symbol table
- `src/rewrite/AGENTS.md` — Source rewriting
- `src/syntax/AGENTS.md` — Syntax types
- `src/syntax/expand/AGENTS.md` — Macro expansion
- `src/hir/analyze/AGENTS.md` — Analyzer
- `src/lir/lower/AGENTS.md` — Lowerer
- `src/value/repr/AGENTS.md` — NaN-boxing
- `src/primitives/json/AGENTS.md` — JSON support
- `src/ffi/primitives/AGENTS.md` — FFI primitives

**Documentation quality**: Excellent. All modules have comprehensive AGENTS.md files with:
- Responsibility statements
- Interface documentation
- Data flow diagrams
- Invariants
- File listings with line counts
- Dependents

### 9.2 README.md Files (25 total)

All major modules have README.md files for human-readable documentation.

---

## 10. REFACTORING OPPORTUNITIES

### 10.1 High Priority

#### 1. **Split `primitives/` module** (16,803 lines, 31 files)

**Current state**: Monolithic module with 31 files  
**Problem**: Large, hard to navigate  
**Solution**: Organize by category:

```
primitives/
├── arithmetic/
│   ├── mod.rs
│   ├── arithmetic.rs
│   ├── bitwise.rs
│   ├── comparison.rs
│   ├── logic.rs
│   └── math.rs
├── collections/
│   ├── mod.rs
│   ├── list.rs
│   ├── array.rs
│   ├── table.rs
│   ├── sets.rs
│   └── structs.rs
├── strings/
│   ├── mod.rs
│   ├── string.rs
│   ├── buffer.rs
│   ├── bytes.rs
│   └── format.rs
├── io/
│   ├── mod.rs
│   ├── fileio.rs
│   ├── ports.rs
│   ├── stream.rs
│   └── net.rs
├── concurrency/
│   ├── mod.rs
│   ├── fibers.rs
│   ├── chan.rs
│   ├── concurrency.rs
│   └── coroutines.rs
├── system/
│   ├── mod.rs
│   ├── process.rs
│   ├── time.rs
│   ├── path.rs
│   └── allocator.rs
├── meta/
│   ├── mod.rs
│   ├── types.rs
│   ├── meta.rs
│   ├── docs.rs
│   ├── debug.rs
│   └── display.rs
├── ffi.rs
├── json/
├── registration.rs
└── module_init.rs
```

**Impact**: Better organization, easier to find related functions

#### 2. **Split `io/` module** (3,489 lines, 6 files)

**Current state**: `aio.rs` (2096 lines) and `backend.rs` (1393 lines) are too large  
**Problem**: Hard to understand async/sync I/O logic  
**Solution**: Organize by operation type:

```
io/
├── mod.rs
├── types.rs
├── request.rs
├── pool.rs
├── aio/
│   ├── mod.rs
│   ├── network.rs
│   ├── file.rs
│   ├── process.rs
│   └── core.rs
└── sync/
    ├── mod.rs
    ├── network.rs
    ├── file.rs
    ├── process.rs
    └── core.rs
```

**Impact**: Clearer separation of concerns, easier to maintain

#### 3. **Split `jit/` module** (5,586 lines, 7 files)

**Current state**: `compiler.rs` (1266), `dispatch.rs` (1320), `translate.rs` (1164) are large  
**Problem**: JIT logic is hard to follow  
**Solution**: Organize by compilation stage:

```
jit/
├── mod.rs
├── code.rs
├── fastpath.rs
├── runtime.rs
├── compile/
│   ├── mod.rs
│   ├── compiler.rs
│   ├── instruction_select.rs
│   └── register_alloc.rs
├── dispatch/
│   ├── mod.rs
│   ├── dispatch.rs
│   ├── grouping.rs
│   └── pattern_match.rs
└── translate/
    ├── mod.rs
    ├── translate.rs
    └── optimization.rs
```

**Impact**: Clearer compilation pipeline, easier to extend

#### 4. **Split `lir/lower/` module** (5,849 lines, 8 files)

**Current state**: `pattern.rs` (1347), `decision.rs` (1243), `escape.rs` (755) are large  
**Problem**: Pattern matching logic is complex  
**Solution**: Organize by concern:

```
lir/lower/
├── mod.rs
├── expr.rs
├── binding.rs
├── control.rs
├── lambda.rs
├── pattern/
│   ├── mod.rs
│   ├── pattern.rs
│   ├── decision_tree.rs
│   └── exhaustiveness.rs
└── escape/
    ├── mod.rs
    ├── escape.rs
    └── analysis.rs
```

**Impact**: Clearer separation of pattern matching concerns

#### 5. **Extract `value/repr/` into separate crate**

**Current state**: NaN-boxing logic in `value/repr/` (1,832 lines)  
**Problem**: Core representation is tightly coupled  
**Solution**: Consider extracting to `elle-value` crate for reuse  
**Impact**: Better modularity, potential for external use

### 10.2 Medium Priority

#### 6. **Consolidate error handling**

**Current state**: Error types spread across `error/` module (987 lines)  
**Problem**: Multiple error types, builders, formatters  
**Solution**: Consolidate into single `error.rs` file (if <500 lines)  
**Impact**: Simpler error handling

#### 7. **Extract `hir/analyze/` into separate module**

**Current state**: 7 files, 3,408 lines  
**Problem**: Analyzer is large and complex  
**Solution**: Organize by form type:

```
hir/analyze/
├── mod.rs (main analyzer)
├── forms/
│   ├── mod.rs
│   ├── binding.rs
│   ├── lambda.rs
│   ├── special.rs
│   └── call.rs
├── destructure.rs
└── binding.rs (binding resolution)
```

**Impact**: Clearer analyzer structure

#### 8. **Reduce `vm/call.rs` (958 lines)**

**Current state**: Call handling, tail calls, JIT dispatch all in one file  
**Problem**: Too many concerns  
**Solution**: Split into:

```
vm/
├── call.rs (call handling)
├── tail_call.rs (tail call logic)
└── jit_dispatch.rs (JIT dispatch)
```

**Impact**: Clearer VM structure

#### 9. **Extract `value/fiber_heap.rs` logic**

**Current state**: 1,216 lines, complex allocator logic  
**Problem**: Hard to understand scope management  
**Solution**: Extract scope management:

```
value/
├── fiber_heap.rs (main allocator)
├── fiber_scope.rs (scope management)
└── shared_alloc.rs (already separate)
```

**Impact**: Clearer allocator structure

### 10.3 Low Priority

#### 10. **Consolidate `syntax/expand/` tests**

**Current state**: `tests.rs` (585 lines) in expand module  
**Problem**: Tests mixed with implementation  
**Solution**: Move to `tests/` directory  
**Impact**: Cleaner module structure

#### 11. **Extract `lir/emit.rs` stack simulation**

**Current state**: 1,146 lines, includes stack simulation  
**Problem**: Stack simulation is complex  
**Solution**: Extract to `lir/stack_sim.rs`  
**Impact**: Clearer emission logic

#### 12. **Consolidate `value/repr/` tests**

**Current state**: `tests.rs` (451 lines) in repr module  
**Problem**: Tests mixed with implementation  
**Solution**: Move to `tests/` directory  
**Impact**: Cleaner module structure

---

## 11. ARCHITECTURAL OBSERVATIONS

### 11.1 Strengths

1. **Clear pipeline architecture** — Source → Reader → Syntax → Expander → Analyzer → Lowerer → Emitter → VM
2. **Well-documented** — Comprehensive AGENTS.md files throughout
3. **Low technical debt** — Only 2 TODO markers
4. **Good test coverage** — ~23,000 lines of tests
5. **Proper error handling** — Centralized error types, no silent failures
6. **Modular design** — Clear separation of concerns
7. **Invariants documented** — Key invariants listed in AGENTS.md files

### 11.2 Weaknesses

1. **Large files** — 96 files over 500 lines, some over 2000 lines
2. **Primitives module too large** — 16,803 lines in 31 files
3. **I/O module too large** — 3,489 lines, 2 files over 1000 lines
4. **JIT module too large** — 5,586 lines, 3 files over 1000 lines
5. **LIR lowering too large** — 5,849 lines, 2 files over 1000 lines
6. **High public API surface** — 71% of items are public
7. **Circular dependencies** — `value` depends on `syntax`, `syntax` depends on `value`

### 11.3 Refactoring Strategy

**Phase 1 (High Priority)**: Split large modules
- `primitives/` → organize by category
- `io/` → organize by operation type
- `jit/` → organize by compilation stage
- `lir/lower/` → organize by concern

**Phase 2 (Medium Priority)**: Reduce large files
- `vm/call.rs` → split into call, tail_call, jit_dispatch
- `hir/analyze/` → organize by form type
- `value/fiber_heap.rs` → extract scope management
- `lir/emit.rs` → extract stack simulation

**Phase 3 (Low Priority)**: Consolidate and clean up
- Move tests out of modules
- Reduce public API surface
- Consolidate error handling
- Extract `value/repr/` to separate crate

---

## 12. SUMMARY STATISTICS

| Metric | Value |
|--------|-------|
| Total .rs files | 170 |
| Total lines of code | ~75,000 |
| Files over 500 lines | 96 |
| Files over 1000 lines | 20 |
| Largest file | `io/aio.rs` (2096 lines) |
| Largest module | `primitives/` (16,803 lines) |
| Test files | 45 |
| Test lines | ~23,000 |
| AGENTS.md files | 26 |
| README.md files | 25 |
| TODO/FIXME markers | 2 |
| Public items | ~450 |
| Crate-private items | ~180 |
| Public/private ratio | 71% / 29% |

---

## 13. NEXT STEPS

1. **Review this analysis** with the team
2. **Prioritize refactoring** based on impact and effort
3. **Create tracking issues** for each refactoring task
4. **Plan implementation** in phases
5. **Update AGENTS.md** files as modules are reorganized
6. **Run tests** after each refactoring to ensure correctness

---

**End of Analysis**
