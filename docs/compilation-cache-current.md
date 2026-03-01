# Compilation Pipeline: Current State

This document records how the compilation pipeline works *before* the caching
changes. It serves as the baseline for understanding what changed and why.

## Pipeline Functions

Seven public functions in `src/pipeline.rs` form the compilation API:

| Function | Signature | Purpose |
|----------|-----------|---------|
| `eval_syntax` | `(Syntax, &mut Expander, &mut SymbolTable, &mut VM) -> Result<Value>` | Compile+execute a Syntax tree using caller's Expander and VM. Used for macro body evaluation. |
| `compile` | `(source, &mut SymbolTable) -> Result<CompileResult>` | Compile one form. Creates internal VM for macro expansion. |
| `compile_all` | `(source, &mut SymbolTable) -> Result<Vec<CompileResult>>` | Compile multiple forms with fixpoint effect inference. Creates internal VM. |
| `eval` | `(source, &mut SymbolTable, &mut VM) -> Result<Value>` | Compile+execute one form using caller's VM. |
| `eval_all` | `(source, &mut SymbolTable, &mut VM) -> Result<Value>` | Compile+execute multiple forms. Delegates to `compile_all` + `vm.execute`. |
| `analyze` | `(source, &mut SymbolTable, &mut VM) -> Result<AnalyzeResult>` | Analyze one form (HIR only, no bytecode). |
| `analyze_all` | `(source, &mut SymbolTable, &mut VM) -> Result<Vec<AnalyzeResult>>` | Analyze multiple forms with fixpoint. |

## What Each Function Creates

### `eval_syntax` (pipeline.rs:91)

Called during macro expansion from `syntax/expand/macro_expand.rs:150`.
Receives the caller's Expander and VM — creates nothing.

Per call:
- `build_primitive_meta(symbols)` — iterates 241 PrimitiveDef entries
- `build_intrinsics(symbols)` — builds intrinsic specialization map
- Analyze → Lower → Emit → Execute

### `compile` (pipeline.rs:119)

Per call:
- `Expander::new()` — empty macro table
- `VM::new()` — fresh fiber, 256-entry globals vec, empty maps
- `register_primitives(&mut macro_vm, symbols)` — ~241 symbol interns (plus
  ~76 aliases), ~317 global sets, ~241 Doc builds, returns PrimitiveMeta
- `expander.load_prelude(symbols, &mut macro_vm)` — parses prelude.lisp
  (176 lines, 13 defmacro forms), expands each (registers macros in Expander)
- `expander.expand(syntax, symbols, &mut macro_vm)` — expands user code;
  macro invocations trigger `eval_syntax` on the internal VM
- Analysis uses PrimitiveMeta from `register_primitives`
- Lower → Emit
- Internal VM and Expander are dropped

### `compile_all` (pipeline.rs:162)

Same setup as `compile`, plus fixpoint iteration for effect inference.

Per call:
- `Expander::new()` + `VM::new()` + `register_primitives` + `load_prelude` (same as `compile`)
- Expand all forms
- Fixpoint loop: analyze all forms, check effect convergence, repeat (max 10 iterations)
- `build_primitive_meta(symbols)` is NOT called directly — uses meta from
  `register_primitives`. However, `eval_syntax` (called during macro expansion)
  does call `build_primitive_meta`.
- Lower + Emit all forms
- Internal VM and Expander are dropped

During expansion, each macro invocation triggers `eval_syntax`, which calls
`build_primitive_meta(symbols)`. A form like `(defn f (x) x)` expands the
`defn` macro, causing one `eval_syntax` call with one `build_primitive_meta`.

### `eval` (pipeline.rs:266)

Uses caller's VM for both macro expansion and execution. No internal VM.

Per call:
- `Expander::new()` + `load_prelude(symbols, vm)` — uses caller's VM for prelude
- `expand(syntax, symbols, vm)` — macro invocations use caller's VM
- `build_primitive_meta(symbols)` — for analyzer
- Analyze → Lower → Emit → `vm.execute`

### `eval_all` (pipeline.rs:298)

Delegates to `compile_all` + execute:
- `compile_all(source, symbols)` — creates internal VM (see above)
- For each result: `vm.execute(&result.bytecode)` — uses caller's VM

### `analyze` (pipeline.rs:313)

Uses caller's VM for expansion. No internal VM.

Per call:
- `Expander::new()` + `load_prelude(symbols, vm)`
- Expand → `build_primitive_meta(symbols)` → Analyze

### `analyze_all` (pipeline.rs:330)

Uses caller's VM for expansion. No internal VM.

Per call:
- `Expander::new()` + `load_prelude(symbols, vm)`
- Expand all forms → `build_primitive_meta(symbols)` → Fixpoint analysis

## The Expander

`Expander` (src/syntax/expand/mod.rs:29) holds:
- `macros: HashMap<String, MacroDef>` — registered macro definitions
- `next_scope_id: u32` — counter for hygiene scopes (starts at 1)
- `expansion_depth: usize` — recursion guard (starts at 0)

`MacroDef` stores: `name: String`, `params: Vec<String>`,
`rest_param: Option<String>`, `template: Syntax`, `definition_scope: ScopeId`.
All string-based — no SymbolIds.

`Expander` does NOT derive `Clone`. It derives nothing. `Default` is
implemented manually (delegates to `new()`).

### `load_prelude`

`Expander::load_prelude(symbols, vm)` parses `prelude.lisp` (embedded via
`include_str!`) and calls `self.expand()` on each syntax form. The prelude
contains 13 `defmacro` definitions:

1. `defn` — function definition shorthand
2. `let*` — sequential bindings
3. `->` — thread-first
4. `->>` — thread-last
5. `when` — conditional body
6. `unless` — inverse conditional
7. `try`/`catch` — error handling via fibers
8. `protect` — run body, capture success/failure
9. `defer` — cleanup after body
10. `with` — resource management
11. `yield*` — delegate to sub-coroutine
12. `ffi/defbind` — FFI function binding
13. `each` — sequence iteration

Each form is a `defmacro`. `handle_defmacro` extracts name/params/body,
creates a `MacroDef`, registers it in `self.macros`. It does NOT call
`fresh_scope()`, does NOT use the VM, does NOT use the SymbolTable. The
VM and SymbolTable parameters are required by `expand()`'s signature but
are not exercised for `defmacro` forms.

After `load_prelude`, the Expander has 13 macro definitions registered.
`next_scope_id` remains at 1. `expansion_depth` remains at 0.

### Macro expansion at use site

When user code uses a prelude macro (e.g., `(defn f (x) x)`):
1. `expand()` sees the call, finds `defn` in `self.macros`
2. `expand_macro_call()` is called
3. A `let`-expression wrapping the macro body is built
4. `pipeline::eval_syntax(let_expr, self, symbols, vm)` compiles and executes it
5. The result Value is converted back to Syntax
6. A fresh scope ID is allocated via `fresh_scope()` for hygiene
7. The result is recursively expanded

This means every macro invocation during expansion triggers a full
compile+execute cycle via `eval_syntax`.

## `init_stdlib`

`init_stdlib` (src/primitives/module_init.rs) calls `pipeline::eval` multiple
times to define Elle-level standard library functions:

```rust
pub fn init_stdlib(vm: &mut VM, symbols: &mut SymbolTable) {
    define_higher_order_functions(vm, symbols);  // 3 eval calls: map, filter, fold
    define_time_functions(vm, symbols);           // 2 eval calls: time/stopwatch, time/elapsed
    define_vm_query_wrappers(vm, symbols);        // 3 eval calls: call-count, global?, fiber/self
    define_graph_functions(vm, symbols);           // 3 eval calls: fn/dot-escape, fn/graph, fn/save-graph
}
```

Each `eval()` call (pipeline.rs:266) creates a fresh `Expander::new()` and
calls `load_prelude`. So `init_stdlib` triggers 11 `load_prelude` calls, each
parsing 176 lines and registering 13 macros.

The `eval()` calls use the caller's VM, so the defined functions (`map`,
`filter`, etc.) end up as closures in the caller's VM globals.

On the current `main` branch (without PR #393's graph functions), `init_stdlib`
makes 8 `eval` calls. PR #393 adds 3 more.

## `register_primitives`

`register_primitives` (src/primitives/registration.rs:49) iterates 31
`PRIMITIVES` tables (241 PrimitiveDef entries, ~76 with aliases), interns each name into
the SymbolTable, sets the global in the VM, and builds Doc entries:

```rust
pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) -> PrimitiveMeta {
    for table in ALL_TABLES {
        for def in *table {
            let sym_id = symbols.intern(def.name);
            vm.set_global(sym_id.0, Value::native_fn(def.func));
            meta.effects.insert(sym_id, def.effect);
            meta.arities.insert(sym_id, def.arity);
            // ... build Doc, handle aliases
        }
    }
    meta
}
```

Returns `PrimitiveMeta { effects, arities, docs }`. The `effects` and `arities`
maps are used by the Analyzer. Note: despite having a `docs` field,
`PrimitiveMeta.docs` is never populated by either `register_primitives` or
`build_primitive_meta` — primitive docs are stored on `vm.docs` instead.

`build_primitive_meta` (registration.rs:91) does the same iteration but only
builds `effects` and `arities` — no VM, no docs (same as `register_primitives`
— neither populates `meta.docs`). Used when a pipeline function receives an
already-configured VM and just needs the metadata.

`SymbolTable::intern` is idempotent: calling it twice with the same name
returns the same `SymbolId`. Primitives are always interned from the same
`ALL_TABLES` array in the same order, so SymbolIds for primitives are
deterministic for any SymbolTable where they're interned before other symbols.

## `build_primitive_meta` Call Sites

Called at these locations (5 call sites):
- `eval_syntax` (pipeline.rs:99) — per macro invocation during expansion
- `eval` (pipeline.rs:277)
- `analyze` (pipeline.rs:322)
- `analyze_all` (pipeline.rs:368)
- `eval_inner` (vm/eval.rs:97) — runtime eval instruction handler

NOT called directly by:
- `compile` — uses meta from `register_primitives` (pipeline.rs:126)
- `compile_all` — uses meta from `register_primitives` (pipeline.rs:166)

However, `eval_syntax` is called during macro expansion inside both `compile`
and `compile_all`, so `build_primitive_meta` is invoked indirectly.

## VM Struct

`VM` (src/vm/core.rs:17) fields:

| Field | Type | Set by register_primitives | Mutated by execution |
|-------|------|---------------------------|---------------------|
| `fiber` | `Fiber` | No | Yes — stack, frames, signal, call_depth |
| `current_fiber_handle` | `Option<FiberHandle>` | No | Yes — fiber switching |
| `current_fiber_value` | `Option<Value>` | No | Yes — fiber switching |
| `globals` | `Vec<Value>` | Yes — primitive NativeFns | Yes — user defs |
| `ffi` | `FFISubsystem` | No | Rarely — FFI calls |
| `loaded_modules` | `HashSet<String>` | No | Yes — import |
| `scope_stack` | `ScopeStack` | No | Yes — scope ops |
| `closure_call_counts` | `FxHashMap<*const u8, usize>` | No | Yes — JIT profiling |
| `location_map` | `LocationMap` | No | Yes — error reporting |
| `tail_call_env_cache` | `Vec<Value>` | No | Yes — reusable buffer |
| `env_cache` | `Vec<Value>` | No | Yes — reusable buffer |
| `pending_tail_call` | `Option<TailCallInfo>` | No | Yes — tail call |
| `current_source_loc` | `Option<SourceLoc>` | No | Yes — error reporting |
| `jit_cache` | `FxHashMap<*const u8, Rc<JitCode>>` | No | Yes — JIT |
| `docs` | `HashMap<String, Doc>` | Yes — primitive docs | No |
| `eval_expander` | `Option<Expander>` | No | Yes — runtime eval caching |

`VM::new()` creates a fresh Fiber (SmallVec stack, empty frames, status=Alive),
globals vec of 256 UNDEFINED values, and empty everything else.

## Thread-Local Context

`src/context.rs` provides thread-local storage for VM and SymbolTable pointers:

- `set_vm_context(vm: *mut VM)` — stores `Some(ptr)` in thread-local
- `set_symbol_table(symbols: *mut SymbolTable)` — stores `Some(ptr)`
- `clear_vm_context()` — stores `None`
- `clear_symbol_table()` — stores `None`

Used by: `gensym` primitive (needs SymbolTable), `length` primitive (needs
SymbolTable for symbol names), runtime `eval` instruction (needs SymbolTable),
`resolve_symbol_name` (display formatting), `prim_import_file` (needs both VM
and SymbolTable context for compilation and execution of imported modules).

## Test Infrastructure

### `eval_source` (tests/common/mod.rs:16)

The canonical test helper. Per call:

```
1. VM::new()
2. SymbolTable::new()
3. register_primitives(&mut vm, &mut symbols)      → PrimitiveMeta (discarded)
4. set_vm_context(&mut vm as *mut VM)               → thread-local Some(ptr)
5. set_symbol_table(&mut symbols as *mut SymbolTable) → thread-local Some(ptr)
6. init_stdlib(&mut vm, &mut symbols)               → 11 eval() calls
7. eval_all(input, &mut symbols, &mut vm)           → compile_all + execute
8. set_vm_context(std::ptr::null_mut())             → thread-local Some(null)
```

Note: step 8 uses `set_vm_context(null_mut)` which stores `Some(null)`, NOT
`clear_vm_context()` which stores `None`. The symbol table context is NOT
cleared — it remains pointing at the (about-to-be-dropped) SymbolTable.

### `setup` (tests/common/mod.rs:35)

Returns `(SymbolTable, VM)` with primitives and stdlib. Sets symbol table
context but does NOT set VM context. Does NOT clear either context after.

### Cost Breakdown for One `eval_source("(+ 1 2)")` Call

| Step | VMs created | register_primitives | load_prelude | build_primitive_meta | build_intrinsics |
|------|-------------|--------------------|--------------|--------------------|-----------------|
| register_primitives | 0 | 1 | 0 | 0 | 0 |
| init_stdlib (11× eval) | 0 | 0 | 11 | 11 | 11 |
| eval_all → compile_all | 1 (internal) | 1 | 1 | 0* | 1 |
| eval_all → vm.execute | 0 | 0 | 0 | 0 | 0 |
| **Total** | **2** | **2** | **12** | **11+** | **12+** |

*`compile_all` uses meta from its `register_primitives`, not `build_primitive_meta`.
But macro invocations during expansion call `eval_syntax` which calls both
`build_primitive_meta` and `build_intrinsics`. For `(+ 1 2)` — no macros,
so 0 additional calls. For `(defn f (x) x)` — 1 macro invocation, 1
additional `build_primitive_meta` + 1 additional `build_intrinsics`.

### Property Tests

15 property test files in `tests/property/`. 10 use `eval_source`:
- `arithmetic.rs`, `bugfixes.rs`, `convert.rs`, `coroutines.rs`,
  `destructuring.rs`, `determinism.rs`, `eval.rs`, `fibers.rs`,
  `macros.rs`, `strings.rs`

5 don't use `eval_source` (work with Rust APIs directly):
- `nanboxing.rs`, `ffi.rs`, `path.rs`, `reader.rs`, `effects.rs`

Case counts per proptest block range from 20 to 1000. CI overrides to 32
via `PROPTEST_CASES` env var.

None of the property tests use stdlib-defined functions (`map`, `filter`,
`fold`, `call-count`, `global?`, `fiber/self`, `time/stopwatch`,
`time/elapsed`, `fn/flow`, `fn/graph`, `fn/save-graph`, `fn/dot-escape`).
They use primitives and prelude macros only.

## Callers of `compile_all`

| Caller | Location | Context |
|--------|----------|---------|
| `eval_all` | pipeline.rs:303 | Test helper and API |
| `run_source` | main.rs:86 | File execution |
| `prim_import_file` | primitives/module_loading.rs:96 | Runtime import |
| Pipeline tests | pipeline.rs (various) | Internal unit tests |

All callers follow the same pattern: `compile_all` returns bytecodes, caller
executes them on their own VM.

## Callers of `compile`

| Caller | Location | Context |
|--------|----------|---------|
| REPL | main.rs:169 | Interactive evaluation |
| Pipeline tests | pipeline.rs (various) | Internal unit tests |

## Callers of `eval`

| Caller | Location | Context |
|--------|----------|---------|
| `define_higher_order_functions` | primitives/higher_order_def.rs:52 | init_stdlib |
| `define_time_functions` | primitives/time_def.rs:23 | init_stdlib |
| `define_vm_query_wrappers` | primitives/module_init.rs:24 | init_stdlib |
| `define_graph_functions` | primitives/graph_def.rs:75 | init_stdlib (PR #393) |

All `eval` callers pass the same (vm, symbols) pair from their caller.

## Callers of `init_stdlib`

| Caller | Location | Context |
|--------|----------|---------|
| `main` | main.rs:380 | Application startup |
| `Linter::lint_str` | lint/cli.rs:51 | Linter per-invocation |
| `CompilerState::new` | lsp/state.rs:54 | LSP server startup |
| `eval_source` | tests/common/mod.rs:24 | Test helper — called per test |
| `setup` | tests/common/mod.rs:40 | Test helper |

Production callers call `init_stdlib` once at startup. Tests call it per
`eval_source` invocation — this is the performance problem.

## Callers of `load_prelude`

| Caller | Location | Creates VM? |
|--------|----------|-------------|
| `compile` | pipeline.rs:127 | Yes (internal) |
| `compile_all` | pipeline.rs:167 | Yes (internal) |
| `eval` | pipeline.rs:274 | No (caller's) |
| `analyze` | pipeline.rs:320 | No (caller's) |
| `analyze_all` | pipeline.rs:337 | No (caller's) |
| `eval_inner` | vm/eval.rs:84 | No (caller's); cached via `vm.eval_expander` |

`vm/eval.rs` caches the Expander on `vm.eval_expander` to avoid repeated
prelude loading for runtime `eval` instructions. This is the only existing
caching of the Expander.

## The `vm.eval_expander` Cache

`vm/eval.rs:79-94`: Runtime `eval` instruction handler takes the Expander
from `vm.eval_expander` (or creates a new one). After expansion, puts it
back. This means the first runtime `eval` loads the prelude; subsequent
ones on the same VM reuse the cached Expander.

This cache is per-VM. It doesn't help with the pipeline-level
`load_prelude` calls (which create their own Expanders).

## Fiber Lifecycle in Compilation VMs

`compile` and `compile_all` create internal VMs for macro expansion.
The Fiber on these VMs is used by `eval_syntax` during macro body
execution. After `compile`/`compile_all` returns, the internal VM
(including its Fiber) is dropped.

The Fiber's state after macro expansion: if all macros expanded
successfully, the Fiber's stack and frames are empty (execution completed
normally). If expansion failed, the Fiber may have residual state (partial
stack, signal set). Either way, the VM is dropped — the state doesn't matter.

## Summary of Redundant Work

For one `eval_source("(defn f (x) x) (f 42)")` call:

| Resource | Created | Per call |
|----------|---------|----------|
| VMs | 2 | 1 execution + 1 compilation (in compile_all) |
| register_primitives | 2 | 1 on execution VM + 1 on compilation VM |
| Expander::new + load_prelude | 12 | 11 from init_stdlib's eval + 1 from compile_all |
| build_primitive_meta | 12+ | 11 from eval + 1+ from macro expansion in compile_all |
| build_intrinsics | 12+ | 11 from eval + 1+ from eval_syntax during expansion |
| Compilation pipelines (parse→emit) | 12+ | 11 from init_stdlib + 1 from compile_all + 1 defn expansion |
| Bytecode executions | 14 | 11 from init_stdlib + 1 from defn macro body + 2 user forms |

The `(defn f (x) x) (f 42)` input has 2 top-level forms. `compile_all`
compiles both, then `eval_all` executes both. During expansion, the `defn`
macro triggers one `eval_syntax` call (macro body execution), adding 1
bytecode execution. Total: 11 + 1 + 2 = 14.

All 11 init_stdlib pipeline runs produce the same functions every time.
The prelude parsing produces the same macros every time. The primitive
metadata is identical every time. None of this varies between test cases.
