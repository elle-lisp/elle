# Compilation Pipeline Caching Plan

## Problem

Every `eval_source()` test call creates fresh VMs, Expanders, and PrimitiveMeta,
doing massive redundant work. On the PR #393 branch, per `eval_source()` call:

- 2 VMs created (1 execution, 1 internal to `compile_all`)
- 2 `register_primitives()` calls (~200 symbol interns + global sets + doc builds)
- 12 `Expander::load_prelude()` calls (11 from init_stdlib's eval, 1 from compile_all)
- `build_primitive_meta()` called per macro invocation during expansion, in `eval`,
  and in `compile_all` — roughly 15+ times per `eval_source`
- 11 full pipeline runs for `init_stdlib` definitions

With ~2,560 property test invocations on CI, this causes timeouts.

## Phase 1: `eval_source_bare` — test-only, zero risk

Add `eval_source_bare()` to `tests/common/mod.rs`. Identical to `eval_source`
including `set_vm_context` and `set_symbol_table` (needed for `gensym`/`length`),
but skips `init_stdlib`. Clears `set_vm_context(std::ptr::null_mut())` after
use — exactly matching `eval_source`'s cleanup (does NOT clear symbol_table
context, matching existing behavior).

Switch 10 property test files to use it. Property tests never use stdlib
functions (verified by grep — zero Elle-level hits for `map`, `filter`, `fold`,
`call-count`, `global?`, `fiber/self`, `time/stopwatch`, `time/elapsed`,
`fn/flow`, `fn/graph`, `fn/save-graph`, `fn/dot-escape`).

Prelude macros (`defn`, `let*`, `->`, `->>`, `when`, `unless`, `try`/`catch`,
`protect`, `defer`, `with`, `yield*`, `each`, `ffi/defbind`) are loaded by
`Expander::load_prelude()` inside `compile_all`, not by `init_stdlib`. They
remain available without `init_stdlib`.

**Files changed:**
- `tests/common/mod.rs` — add `eval_source_bare()`
- 10 property test files switch to `eval_source_bare`:
  `arithmetic.rs`, `bugfixes.rs`, `convert.rs`, `coroutines.rs`,
  `destructuring.rs`, `determinism.rs`, `eval.rs`, `fibers.rs`,
  `macros.rs`, `strings.rs`
- 5 property test files unchanged (don't use `eval_source`):
  `nanboxing.rs`, `ffi.rs`, `path.rs`, `reader.rs`, `effects.rs`

**Eliminates:** ~28,160 full pipeline runs from `init_stdlib` per test suite.

## Phase 2+4: `CompilationCache` — cached compilation VM + Expander

Derive `Clone` on `Expander`. It's three fields:
- `macros: HashMap<String, MacroDef>` — MacroDef is already Clone
- `next_scope_id: u32` — Copy
- `expansion_depth: usize` — Copy

Add `VM::reset_fiber()` to `src/vm/core.rs`:
```rust
pub fn reset_fiber(&mut self) {
    self.fiber = Fiber::new(root_closure(), 0);
    self.fiber.status = FiberStatus::Alive;
    self.current_fiber_handle = None;
    self.current_fiber_value = None;
    self.pending_tail_call = None;
    self.current_source_loc = None;
    self.scope_stack = ScopeStack::new();
    self.closure_call_counts.clear();
    self.location_map = LocationMap::new();
    self.loaded_modules.clear();
    // Preserved: globals (primitives), docs, ffi, jit_cache,
    //   eval_expander, env_cache, tail_call_env_cache
}
```

Create a `thread_local! CompilationCache` in `pipeline.rs` holding:
- **VM** with primitives registered (fiber always reset between uses)
- **Expander** with prelude loaded (cloned for each pipeline call)
- **PrimitiveMeta** from `register_primitives` (for compile/compile_all's analyzer)

Initialized once per thread with a throwaway SymbolTable. The throwaway VM has
`register_primitives` called on it for safety. `compile` and `compile_all`
borrow the VM (after `reset_fiber`), clone the Expander, and use the meta.
`eval`, `analyze`, `analyze_all` only clone the Expander (they use the caller's
VM for expansion).

**Why cloning the Expander is safe:**
- Each clone starts with `next_scope_id` at the prelude-loaded value
  (confirmed: prelude is 100% defmacro, `handle_defmacro` doesn't call
  `fresh_scope()`, templates are stored raw not expanded at definition time)
- Subsequent expansion increments the clone's scope ID independently
- Scope IDs only need uniqueness within a single expansion session
- Cloned Expanders in different pipeline calls are independent sessions
- The Expander stores strings, not SymbolIds — it's SymbolTable-independent

**Non-re-entrancy:** Top-level pipeline functions (`compile`, `compile_all`,
`eval`, `analyze`, `analyze_all`) are never called from within each other.
`eval_syntax` (called during expansion) receives its own `&mut Expander` and
doesn't touch the cache. The `CompilationCache` borrow is never re-entrant.

**Safety net:** Verify first primitive's SymbolId matches on cache hit; fall
back to fresh VM on mismatch.

**Preserved across `reset_fiber`:**
- `globals` — NativeFn values from register_primitives. Never mutated by macro expansion.
- `docs` — built by register_primitives. Never mutated by macro expansion.
- `ffi` — FFISubsystem. Macro expansion doesn't use FFI.
- `jit_cache` — empty (no JIT during compilation).
- `eval_expander` — caches Expander for runtime `eval` inside macro bodies.
  Safe: its mutable state is `expansion_depth` (reset to 0 after each call)
  and macro definitions (immutable after prelude loading).
- `env_cache`, `tail_call_env_cache` — reusable allocation buffers.

**SymbolId consistency:**
`register_primitives` interns primitives into a throwaway SymbolTable, producing
IDs 0..N deterministically (sequential assignment, idempotent). When `compile_all`
is called with the caller's SymbolTable, the caller has also called
`register_primitives`, producing the same IDs (intern is idempotent and
primitives are always registered in the same order from `ALL_TABLES`). The
cached VM's globals (indexed by SymbolId) are therefore always consistent.

**Files changed:**
- `src/syntax/expand/mod.rs` — `#[derive(Clone)]` on `Expander`
- `src/vm/core.rs` — `VM::reset_fiber()` method
- `src/pipeline.rs` — `CompilationCache` thread_local; modify `compile`,
  `compile_all`, `eval`, `analyze`, `analyze_all` to use cached Expander/VM

**Eliminates:** 1 VM creation + 1 `register_primitives` + 12 prelude parsings
per `eval_source`.

## Phase 3: `cached_primitive_meta` — separate cache

`build_primitive_meta` is called by `eval_syntax` (every macro invocation during
expansion), `eval`, `analyze`, `analyze_all`, and `vm/eval.rs`. Each call
iterates 241 PrimitiveDef entries (plus ~76 aliases) and interns symbol names.

Add `thread_local! PRIMITIVE_META_CACHE` in `registration.rs`. Once populated,
cached forever within the thread (primitive metadata never changes). Returns
clone on hit, populates on miss.

**Precondition:** Callers must have already interned primitives in their
SymbolTable. True for all current call sites — they all go through
`register_primitives` first (either directly or via `compile_all`'s internal
setup). The cache hit path skips the `intern` side-effect; this is safe because
the caller's SymbolTable already has all primitive symbols.

Replace `build_primitive_meta(symbols)` at 5 call sites:
- `pipeline.rs`: `eval_syntax` (line 99), `eval` (line 277), `analyze`
  (line 322), `analyze_all` (line 368)
- `vm/eval.rs`: `eval_inner` (line 97)

Note: `compile` and `compile_all` get their meta from `register_primitives()`'s
return value, not from `build_primitive_meta`. But `eval_syntax` — called during
macro expansion inside `compile`/`compile_all` — does call `build_primitive_meta`.

**Files changed:**
- `src/primitives/registration.rs` — `PRIMITIVE_META_CACHE` + `cached_primitive_meta()`
- `src/pipeline.rs` — replace at 4 call sites
- `src/vm/eval.rs` — replace at 1 call site

**Eliminates:** ~15+ × 241 PrimitiveDef iterations per `eval_source`.

## Implementation Order

1. Phase 1 (test-only, independent)
2. Phase 2+4 (`Expander` Clone + `CompilationCache` + `reset_fiber`)
3. Phase 3 (`cached_primitive_meta`)

## File Change Summary

| File | What |
|------|------|
| `tests/common/mod.rs` | `eval_source_bare()` |
| 10 `tests/property/*.rs` | Switch to `eval_source_bare` |
| `src/syntax/expand/mod.rs` | `#[derive(Clone)]` on `Expander` |
| `src/vm/core.rs` | `VM::reset_fiber()` |
| `src/pipeline.rs` | `CompilationCache` thread_local; modify 5 pipeline fns |
| `src/primitives/registration.rs` | `PRIMITIVE_META_CACHE` + `cached_primitive_meta()` |
| `src/vm/eval.rs` | Use `cached_primitive_meta()` |

## Risks

| Risk | Mitigation |
|------|------------|
| Thread safety of Rc in caches | Thread_local — no sharing across threads |
| Stale Expander cache | Prelude is include_str! — changes only on recompile |
| Compilation VM state leakage | `reset_fiber()` always called; globals immutable |
| SymbolId mismatch | Verify first primitive ID on cache hit; fall back on miss |
| Future prelude with non-defmacro forms | Debug assertion: all prelude forms are defmacro |
| ScopeId collision across cloned Expanders | Harmless — independent sessions |
| `cached_primitive_meta` skips intern side-effect | Callers already have primitives interned |
| `eval_expander` preserved across reset | Safe: only mutable state is expansion_depth (auto-reset) |

## Invariants Introduced

These must remain true or the caches break:

1. **Prelude is 100% defmacro.** No runtime definitions. If a non-defmacro form
   is added, it would execute in the throwaway VM during cache initialization,
   not in the caller's VM — producing incorrect results.

2. **Primitives are registered before any pipeline function is called.** The
   `cached_primitive_meta` cache relies on primitive symbols already being
   interned in the caller's SymbolTable.

3. **`compile_all` and `compile` are top-level entry points.** They are not
   called from within macro expansion or from within each other. The
   `CompilationCache` borrow is not re-entrant.

4. **Primitive registration order is deterministic.** `ALL_TABLES` order +
   sequential `SymbolId` assignment means the same primitives always get the
   same IDs. The cached compilation VM's globals depend on this.
