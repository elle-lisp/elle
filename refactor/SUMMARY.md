# Elle 2.0 Refactoring Synthesis

> **Status as of February 2025**: Phases 1 (NaN-boxing), 3 (Syntax), 4 (HIR),
> 5 (LIR), and 6 (Scope) are complete. Phase 2 (Error system) has not started.
> Phase 7 (Semantics) is partially done. Phase 8 (Cleanup) is in progress —
> the old compilation pipeline is being removed. See divergences below.

## Summary of Agent Recommendations

Five agents reviewed the codebase with different perspectives:

| Agent | Focus | Key Themes |
|-------|-------|------------|
| **R1** | Architectural purity | CPS as canonical IR, arena allocation, NaN-boxing, phase separation |
| **R2** | Module organization | Split large files, break circular dependencies, clear compilation pipeline |
| **R3** | Pragmatic execution | Bytecode VM primary, JIT supplementary, complete what exists |
| **R4** | Implementation detail | Lexical scope fix, loop scoping, closure environments, dead code cleanup |
| **R5** | Type-level changes | Unified error system, effect system completion, tiered JIT, module redesign |

---

## Consensus Areas (All or Most Agents Agree)

### 1. **Value Representation: NaN-Boxing**
All agents agree `Value` is oversized (24 bytes, 26+ variants) and should be NaN-boxed to 8 bytes. This also subsumes:
- Merging `Cell`/`LocalCell` into one type
- Merging `Closure`/`JitClosure` into one type
- Removing `Exception` variant (use `Condition` only)

**Status: ✅ COMPLETE** (merged in `value-nan-boxing` branch, Feb 2025)

What shipped vs. what was planned:
- Cell unification used `HeapObject::Cell(RefCell<Value>, bool)` where the bool
  distinguishes local (auto-unwrap) from user (explicit) cells. The plan proposed
  a `cell_mask` bitmap on `Closure` — the shipped approach puts metadata on the
  data rather than the consumer. Works but is architecturally less clean.
- `Closure`/`JitClosure` merge did NOT happen. Both still exist in `value_old`.
  This is blocked on removing `value_old` (separate PR).
- `Exception` variant removed from new `HeapObject`. Old `value_old::Value` still
  has it. Will be cleaned up when `value_old` is removed.

### 2. **Unified Error System**
All agents identify `Result<T, String>` pervasive throughout the codebase as a defect. `LError` exists but isn't used. Migration scope: ~200 function signatures, ~50 files.

**Status: ❌ NOT STARTED**

The error system diverged from plan. Instead of unified `LError`:
- `NativeFn` returns `Result<Value, Condition>` (user-facing errors)
- `VmAwareFn` sets `vm.current_exception` directly, returns `LResult<Value>`
- Two error channels exist: `Err(String)` = VM bug (uncatchable),
  `vm.current_exception` = runtime error (catchable by `handler-case`)
- This two-channel design is documented in `docs/EXCEPT.md`

The unified `LError` approach may still be worth pursuing, but the current
system works and is well-documented. Revisit after old pipeline removal.

### 3. **Compilation Pipeline Separation**
All agents agree parsing, macro expansion, semantic analysis, and codegen are improperly interleaved. Consensus on at least:
- New `Syntax` type for S-expression AST before analysis
- Clear macro expansion phase operating on `Syntax`
- `Expr` as post-analysis only

**Status: ✅ COMPLETE**

The new pipeline is:
```
Source → Reader → Syntax → Expander → Syntax → Analyzer → HIR → Lowerer → LIR → Emitter → Bytecode → VM
```

Key divergence from plan: `Expr` was NOT retained as an intermediate. The plan
proposed `Syntax → Expr → HIR → LIR`. What shipped goes directly from `Syntax`
to `HIR`, skipping `Expr` entirely. `Expr` is the old pipeline's AST type
(`compiler/ast.rs`) and is being removed.

Module locations differ from plan:
- `src/hir/` (not `src/compiler/hir/`)
- `src/lir/` (not `src/compiler/lir/`)
- `src/syntax/` (not `src/compiler/syntax/`)
- `src/pipeline.rs` orchestrates the full flow

### 4. **Scope/Variable Resolution Cleanup**
All agents identify the `VarRef` system as broken or confusing:
- Three scope systems that don't interoperate
- `Local`/`LetBound` distinction unclear
- Loop variables stored in globals
- Closure captures are static snapshots, not references

**Status: ✅ COMPLETE** (in new pipeline)

The new pipeline uses `BindingId` throughout — no `VarRef` at all. The old
`VarRef` system exists only in the legacy pipeline being removed. Loop variables
are properly scoped as locals. Closure captures use `LocalCell` for mutated
variables.

### 5. **Dead Code Removal**
~22 files of obsolete Cranelift milestones and compiler versions.

**Status: 🔄 IN PROGRESS**

The old compilation pipeline (`compiler/converters/`, `compiler/compile/`,
`compiler/ast.rs`, `compiler/scope.rs`, `compiler/analysis.rs`,
`compiler/capture_resolution.rs`) is being removed in the `remove-old-pipeline`
branch. This also removes ~35 old-pipeline test files. Cranelift milestone
files (~22 files) have not been audited yet.

### 6. **Large File Modularization**
`compile/mod.rs` (1200-1800 lines), `cranelift/compiler.rs` (1900 lines), `vm/mod.rs` (980+ lines) all violate the 300-line target.

---

## Divergence Areas

| Topic | Agent Views | Resolution |
|-------|-------------|------------|
| **Canonical IR** | R1: CPS everywhere. R3: Bytecode VM primary, CPS for coroutines only | Given elegance priority: **CPS as canonical**, bytecode VM as fallback for non-yielding code |
| **JIT strategy** | R1: Cranelift for hot loops. R3: JIT supplementary. R5: Tiered compilation | **Defer tiered JIT to 3.0**; unify `Closure`/`JitClosure` now |
| **Module system** | R4: Detailed spec. R5: First-class modules. R3: Finish what exists | **Finish existing module scaffolding** with explicit exports |
| **Effect system** | R1: Effect inference in analyze phase. R5: Compile-time enforcement. R3: Effects stored but not enforced | **Complete effect enforcement** at compile time |

---

## Prioritized Recommendations

### PRIORITY 1: Do Now (Foundation)

These must happen first because everything else depends on them.

#### 1.1 NaN-Boxed Value Representation
**Sources**: R1, R2, R3, R5
**Status: ✅ COMPLETE**

8-byte NaN-boxed values with tagged pointers for heap objects. Immediate encoding for:
- Nil, Bool, Int (i48), Symbol, small floats

Heap allocation for:
- Cons, Closure, String, Vector, Table, etc.

**Key changes**:
- `Value` becomes `Copy` (no more `.clone()` everywhere)
- Pattern matching via `value.as_int()`, `value.as_closure()` methods
- Single `Closure` type with optional `jit_code: Option<*const u8>`
- Single `Cell` type with `mutable: bool` flag

**Estimated scope**: Value.rs rewrite, all VM instruction handlers, all primitives.

#### 1.2 Compilation Pipeline: Syntax → Expr → HIR → LIR → Bytecode
**Sources**: R1, R2, R5
**Status: ✅ COMPLETE**

Full phase separation:

```
Source → Lexer → Tokens
Tokens → Parser → Syntax (S-expression AST, pre-macro)
Syntax → MacroExpand → Syntax (fully expanded)
Syntax → Analyze → HIR (typed, scopes resolved, effects inferred)
HIR → Lower → LIR (SSA form, close to machine)
LIR → Emit → Bytecode or Cranelift
```

**Key changes**:
- New `Syntax` type: S-expression AST before semantic analysis
- New `HIR` type: typed expressions with resolved bindings
- New `LIR` type: SSA form for optimization passes
- Macros operate on `Syntax`, not `Value`
- CPS transformation operates on HIR
- Single analysis shared by bytecode and JIT backends

**Estimated scope**: New `src/compiler/hir/` and `src/compiler/lir/` modules, refactor `converters/`.

#### 1.3 Unified Error System
**Sources**: R1, R2, R3, R4, R5
**Status: ❌ NOT STARTED** — diverged, see above

Replace all `Result<T, String>` with `Result<T, LError>`:

```rust
pub type NativeFn = fn(&[Value], &mut ErrorCtx) -> EllResult<Value>;

pub struct LError {
    pub kind: ErrorKind,
    pub location: SourceLoc,
    pub stack_trace: Vec<CallFrame>,
}
```

**Key changes**:
- All primitives get new signature
- All VM execution returns `LError`
- Source location attached at error creation
- Stack traces captured automatically

**Estimated scope**: ~200 function signatures, ~50 files.

#### 1.4 Lexical Scope Fix
**Sources**: R4 (detailed spec), R2, R3
**Status: ✅ COMPLETE** (in new pipeline)

The current scope system is broken. Three incompatible systems must become one:

```rust
pub enum VarRef {
    Local { index: usize, cell_boxed: bool },
    Upvalue { index: usize, cell_boxed: bool },
    Global { sym: SymbolId },
}
```

**Key changes**:
- `BindingResolver` assigns indices at compile time
- Remove runtime `ScopeStack` from VM
- Loop variables scoped to loop body (not globals)
- Closure captures become `Rc<RefCell<Value>>` for mutated variables

**Estimated scope**: 9-12 days per R4's detailed breakdown.

---

### PRIORITY 2: Do Next (Semantics)

After foundation is stable, complete the semantic model.

#### 2.1 Exception/Condition System Completion
**Sources**: R1, R3

- Remove `Exception` variant from Value
- Implement `InvokeRestart` bytecode instruction
- Move exception hierarchy from hardcoded match to data-driven registry
- `handler-bind` attaches non-unwinding handlers

**Status: ⚠️ PARTIAL**
- `handler-case` (unwinding): complete
- `handler-bind` (non-unwinding): stub — parsed but codegen ignores handlers
- Restarts: unimplemented — `InvokeRestart` opcode allocated, VM handler is no-op
- `signal`/`warn`/`error` primitives: misnamed constructors, don't actually signal
- `try`/`catch`/`finally`: DEAD — excised, conditions are the system now

#### 2.2 Effect System Enforcement
**Sources**: R1, R5

- Effect checking at compile time (pure functions can't call impure)
- Effect annotations in Elle syntax: `(defn foo :pure (x) ...)`
- Effect polymorphism: `(defn map :effect-of(f) ...)`

**Status: ❌ NOT STARTED** — effects inferred but not enforced

#### 2.3 Module System Completion
**Sources**: R4, R5, R3

Finish existing scaffolding:
- `Import` compilation (currently emits `Nil`)
- Module-qualified symbol resolution
- Explicit exports with `(module name :export [...])`
- Circular import detection

**Status: ❌ NOT STARTED** — module_loading.rs still uses old pipeline

---

### PRIORITY 3: Do Later (Performance & Polish)

After semantics are correct, optimize.

#### 3.1 CPS/Coroutine Unification
**Sources**: R1, R3, R5

- CPS as canonical for all yielding code
- Remove bytecode-based coroutine path
- Foundation for async/await

**Status: ⚠️ PLANNED** — see `docs/CPS_REWORK.md` for design. Not started.

#### 3.2 Bytecode Format Redesign
**Sources**: R3, R5

Fixed-width 32-bit instructions:
- 8-bit opcode, 24-bit operands
- 65535 locals/upvalues/constants (up from 255)
- Source span integration for debugging

**Status: ❌ NOT STARTED**

#### 3.3 Arena-Based Memory
**Sources**: R1

- Region allocator for short-lived allocations
- Generational collection: nursery → traced heap
- Deterministic GC pauses

**Status: ❌ NOT STARTED**

---

### FUTURE WORK (Defer to 3.0)

These have merit but don't belong in this cycle.

| Item | Reason to Defer |
|------|-----------------|
| **Tiered JIT compilation** (R5) | Requires stable foundation first; premature optimization |
| **Parser byte-based lexer** (R4) | Performance optimization, not semantic improvement |
| **File I/O abstraction layer** (R4) | Nice for testing, not essential for language correctness |
| **Proc-macro primitive registration** (R5) | Convenience, not blocking |
| **Documentation generation** (R5) | Polish phase, after APIs stabilize |
| **FFI layered redesign** (R4, R2) | FFI is noted as non-functional; fix basic FFI first |
| **Symbol table architecture** (R5) | Low impact per R5's own ranking |
| **Test infrastructure improvements** (R5) | Ongoing work, not gating |

---

## Execution Order

Given priorities (elegance first, NaN-boxing included, full pipeline):

### Phase 1: Value & Errors (Weeks 1-3) — ✅ VALUE DONE, ❌ ERRORS NOT STARTED
1. NaN-boxing Value representation
2. Unified error system (`LError` everywhere)
3. Merge `Cell`/`LocalCell`, `Closure`/`JitClosure`

### Phase 2: Compilation Pipeline (Weeks 4-8) — ✅ COMPLETE
4. `Syntax` type + parser producing it
5. Macro expansion on `Syntax`
6. HIR type + analyzer
7. LIR type + lowering
8. Bytecode emission from LIR

### Phase 3: Scope & Semantics (Weeks 9-12) — ✅ SCOPE DONE, ⚠️ SEMANTICS PARTIAL
9. `BindingResolver` + unified `VarRef`
10. Loop variable scoping fix
11. Closure environment references (not snapshots)
12. Exception/condition system completion
13. Effect system enforcement

### Phase 4: Cleanup (Weeks 13-14) — 🔄 IN PROGRESS
14. Dead code removal (~22 files)
15. Large file modularization
16. Module system completion

---

## Critical Path Dependencies

```
NaN-boxing Value ─────────────────────────────────────────┐
                                                          │
Unified Error System ─────────────────────────────────────┤
                                                          │
Syntax type ──→ Macro expansion on Syntax ──→ HIR type ──┼──→ LIR type ──→ Bytecode from LIR
                                                          │
                                   BindingResolver ───────┤
                                                          │
                              Lexical scope fix ──────────┘
                                     │
                              Loop scoping fix
                                     │
                              Closure env fix
```

The NaN-boxing and Syntax type can proceed in parallel. HIR depends on both. Everything downstream depends on HIR.

> **Update**: The dependency graph played out differently. NaN-boxing, Syntax,
> HIR, LIR, and scope were completed. The error system was skipped. The old
> pipeline coexisted with the new one until now. Current work: removing the
> old pipeline, then `value_old`, then addressing remaining semantic gaps.

---

## Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|-------------|--------|------------|
| NaN-boxing breaks everything | High | Critical | Branch; comprehensive test suite before/after |
| HIR/LIR introduces new bugs | Medium | High | Incremental: HIR works before starting LIR |
| Scope fix breaks existing code | High | High | Run all `tests/` + `examples/` after each phase |
| Timeline slips | High | Medium | Prioritize correctness over schedule |

---

## Current Work (February 2025)

### Active: Old Pipeline Removal (`remove-old-pipeline` branch)
- New pipeline property tests written to cover all semantic categories
- Runtime consumers being migrated: `module_loading.rs`, `resident_compiler`,
  `higher_order_def.rs`
- Old pipeline code to delete: `compiler/converters/`, `compiler/compile/`,
  `compiler/ast.rs`, `compiler/scope.rs`, `compiler/analysis.rs`,
  `compiler/capture_resolution.rs`
- ~35 old-pipeline test files to delete (coverage replaced by new tests)
- `try`/`catch`/`finally` being excised entirely

### Next: `value_old` Removal (separate PR)
- Move ~17 type definitions from `value_old/mod.rs` into `value/` submodules
- Eliminate old `Value` enum and conversion bridges
- Reconcile two `Condition` types

### Blocked on `value_old` removal:
- `Closure`/`JitClosure` merge
- Full `Exception` variant removal

### Remaining semantic work:
- `handler-bind` implementation (non-unwinding handlers)
- Signal/restart system (or decision to not implement)
- Effect enforcement at compile time
- Module system completion
- CPS rework (`docs/CPS_REWORK.md`)

---

## Remaining Recommendations

**Do now** (in progress):
1. Remove old pipeline
2. Remove `value_old` (move types to `value/`)

**Do next** (semantic completion):
3. Unified error system (or formalize current two-channel approach)
4. `handler-bind` + signal/restart (or decide against CL-style conditions)
5. Effect enforcement
6. Module system completion

**Do later** (performance):
7. CPS rework for coroutines
8. Bytecode format redesign
9. Arena-based memory

**Defer to 3.0**:
- Tiered JIT
- Parser optimizations
- FFI redesign
