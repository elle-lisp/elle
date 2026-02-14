# Lexical Scope Refactoring Plan

## Overview

This document describes a comprehensive refactoring of Elle's variable scoping system to implement proper lexical scope. The current implementation has three different scope mechanisms that need unification under a single, compile-time resolved model.

## Current Problems

### 1. Dual Scope Systems

- **Bytecode VM**: Uses `ScopeStack` with runtime scope chain lookup (effectively dynamic scoping)
- **CPS Interpreter**: Uses flat index-based environments with `Rc<RefCell<Vec<Value>>>`

The bytecode VM's `ScopeStack` walks up scopes at runtime to find variables. The CPS system correctly uses compile-time resolved indices. These don't share a coherent model.

### 2. Environment Layout Inconsistency

The closure environment is `[captures..., params..., locals...]`, but tracking is fragmented:
- `ScopeEntry` in `converters/` uses symbol lists
- `capture_resolution.rs` post-processes to fix indices  
- `compile/mod.rs` uses `lambda_locals`, `lambda_captures_len`, `lambda_params_len` separately
- CPS transformer has its own `next_local_index` and `local_indices` HashMap

### 3. Variable Representation Chaos

`Expr::Var(sym, depth, index)` is overloaded:
- `depth` sometimes means "scope stack depth" (for bytecode)
- `depth` sometimes means "lambda nesting depth" (for closures)
- `index` sometimes means "parameter position" or "slot in environment"

`Expr::GlobalVar` vs `Expr::Var` with `index == usize::MAX` adds more confusion.

### 4. Let Variables Using Runtime Scope

From `value_to_expr.rs`: let-bound variables outside lambdas become `GlobalVar`, using runtime scope lookup instead of compile-time resolution.

## Design Decisions

### VarRef: Unified Variable Representation

Replace `Expr::Var` and `Expr::GlobalVar` with:

```rust
/// A resolved variable reference - all resolution happens at compile time
#[derive(Debug, Clone, PartialEq)]
pub enum VarRef {
    /// Local variable in current activation frame
    /// index is offset in frame's locals array
    Local { index: usize },
    
    /// Captured variable from enclosing closure
    /// index is offset in closure's captures array  
    Upvalue { index: usize },
    
    /// Global/top-level binding
    /// sym is used for runtime lookup in globals HashMap
    Global { sym: SymbolId },
}
```

**Rationale**: Clear semantic distinction - you know exactly what runtime operation is needed. No runtime interpretation of depth. Upvalues are accessed from the closure's capture array, not by walking scope chains.

### Cell Boxing for Mutated Captures

When a variable is captured by a nested lambda AND mutated via `set!`, it needs cell boxing (`Rc<RefCell<Value>>`).

**Approach**: Separate bytecode instructions handle boxed vs unboxed access:
- `LoadLocal` vs `LoadLocalCell`
- `LoadUpvalue` vs `LoadUpvalueCell`
- `StoreLocal` vs `StoreLocalCell`
- `StoreUpvalue` vs `StoreUpvalueCell`

The compiler determines which to emit based on static analysis. No runtime type checks.

### Bytecode Instructions

New instruction set for variable access:

```
LoadLocal <index>        - Push locals[index] to stack
LoadUpvalue <index>      - Push captures[index] to stack
LoadGlobal <sym_id>      - Push globals[sym_id] to stack

StoreLocal <index>       - Pop and store to locals[index]
StoreUpvalue <index>     - Pop and store to captures[index]
StoreGlobal <sym_id>     - Pop and store to globals[sym_id]

LoadLocalCell <index>    - Push unwrap(locals[index]) to stack
LoadUpvalueCell <index>  - Push unwrap(captures[index]) to stack
StoreLocalCell <index>   - Pop and store into cell at locals[index]
StoreUpvalueCell <index> - Pop and store into cell at captures[index]
```

**Removed instructions**:
- `PushScope` / `PopScope` - Scopes are compile-time only
- `DefineLocal` - Locals are pre-allocated in the frame
- `LoadUpvalue` with depth parameter - Replaced by explicit Local/Upvalue distinction

### Global Variables

Globals remain in `HashMap<u32, Value>` (current approach). Rationale:
- Already O(1) hash lookup
- REPL/interactive use requires dynamic global definition
- Module system can be layered on later

## What Gets Eliminated

1. **`vm/scope/`** - Entire directory (`ScopeStack`, `RuntimeScope`, handlers)
2. **`PushScope` / `PopScope` bytecode instructions**
3. **`DefineLocal` instruction**
4. **Runtime scope chain traversal**

## Implementation Phases

### Phase 1: Cranelift Cleanup

Move dead code to `trash/cranelift/`:

**Files to move** (13 phase milestones + 9 other dead files):
- `phase3_milestone.rs` through `phase15_milestone.rs` (13 files)
- `compiler_v2.rs`, `compiler_v3.rs`, `compiler_v3_stack.rs`, `compiler_v4.rs`
- `adaptive_compiler.rs`, `advanced_optimizer.rs`, `escape_analyzer.rs`
- `feedback_compiler.rs`, `type_specializer.rs`
- `tests.rs`, `e2e_test.rs`
- `expr_compiler.rs`, `function_compiler.rs`, `closure_compiler.rs`, `optimizer.rs`

**Update `mod.rs`**: Remove all references to moved files.

### Phase 2: Create Binding Module

New module `src/binding/` with:

```
src/binding/
├── mod.rs           # Public API
├── resolver.rs      # BindingResolver implementation
├── varref.rs        # VarRef enum definition
└── analysis.rs      # Free var, capture, mutation analysis
```

**BindingResolver responsibilities**:
1. Track bindings during AST construction
2. Assign indices to all locals at compile time
3. Identify which locals need cell-boxing
4. Produce `VarRef` for every variable reference

### Phase 3: Update AST

Modify `src/compiler/ast.rs`:

```rust
// Remove these:
// Var(SymbolId, usize, usize),
// GlobalVar(SymbolId),

// Add this:
Var(VarRef),

// Update Set:
Set {
    target: VarRef,
    value: Box<Expr>,
},
```

### Phase 4: Update Bytecode Compiler

Modify `src/compiler/compile/mod.rs`:
1. Remove `lambda_locals`, `lambda_captures_len`, `lambda_params_len` tracking
2. Use `BindingResolver` instead
3. Emit new instruction variants based on `VarRef`
4. Remove scope depth tracking

### Phase 5: Update Bytecode Instructions

Modify `src/compiler/bytecode.rs`:
1. Add new instruction variants
2. Remove `PushScope`, `PopScope`, `DefineLocal`
3. Remove depth parameter from upvalue instructions

### Phase 6: Update VM

Modify `src/vm/`:
1. Remove `scope/` directory entirely
2. Update `core.rs` to use flat locals array per frame
3. Update instruction handlers for new variants
4. Remove scope-related state from `VM` struct

### Phase 7: Update CPS

Modify `src/compiler/cps/`:
1. Update `transform.rs` to use `BindingResolver`
2. Update `interpreter.rs` to use `VarRef`
3. Remove redundant scope tracking

### Phase 8: Update Converters

Modify `src/compiler/converters/`:
1. Update `value_to_expr.rs` to use `BindingResolver`
2. Remove `ScopeEntry` type
3. Remove `ScopeType` enum (or move to binding module if needed)

### Phase 9: File Splitting

Split large files after core refactoring is stable:

**`compile/mod.rs` (1633 lines) → Split into:**
- `compile/mod.rs` - Compiler struct and top-level API
- `compile/emitter.rs` - Instruction emission
- `compile/control.rs` - If/while/for compilation
- `compile/lambda.rs` - Lambda/closure compilation
- `compile/literals.rs` - Constant handling

**`vm/mod.rs` (986 lines) → Already has submodules, just slim down**

**`cps/interpreter.rs` (1000 lines) → Split into:**
- `cps/interpreter.rs` - Core eval function
- `cps/eval_control.rs` - If/while/sequence evaluation
- `cps/eval_call.rs` - Function call handling

## Testing Strategy

### Unit Tests

Each phase should have tests verifying:
1. Variable resolution produces correct `VarRef`
2. Cell boxing is applied correctly
3. Bytecode emission matches expected instructions

### Integration Tests

Existing tests in `tests/` should continue passing. Key scenarios:
- Nested lambdas with captures
- Mutated captured variables
- Let bindings at various nesting levels
- Recursive functions
- Coroutines with yields across scopes

### Regression Prevention

Before each phase:
1. Run full test suite
2. Run benchmarks to detect performance regressions
3. Test REPL interactively

## File Inventory

### Files to Delete/Move

| File | Destination | Reason |
|------|-------------|--------|
| `src/vm/scope/` | Delete | Runtime scope eliminated |
| `src/compiler/scope.rs` | Move to `binding/` or delete | ScopeType may be useful |
| `src/compiler/cranelift/phase*.rs` | `trash/cranelift/` | Dead milestone tests |
| `src/compiler/cranelift/compiler_v*.rs` | `trash/cranelift/` | Unused compiler versions |
| Various cranelift files | `trash/cranelift/` | See Phase 1 list |

### Files to Create

| File | Purpose |
|------|---------|
| `src/binding/mod.rs` | Binding module public API |
| `src/binding/resolver.rs` | BindingResolver implementation |
| `src/binding/varref.rs` | VarRef enum |
| `src/binding/analysis.rs` | Static analysis functions |

### Files to Heavily Modify

| File | Changes |
|------|---------|
| `src/compiler/ast.rs` | New Var/Set variants |
| `src/compiler/bytecode.rs` | New instructions |
| `src/compiler/compile/mod.rs` | Use BindingResolver |
| `src/compiler/converters/value_to_expr.rs` | Use BindingResolver |
| `src/compiler/cps/transform.rs` | Use BindingResolver |
| `src/compiler/cps/interpreter.rs` | Use VarRef |
| `src/vm/mod.rs` | Remove scope stack usage |
| `src/vm/core.rs` | Flat locals per frame |
| `src/vm/variables.rs` | New instruction handlers |
| `src/value.rs` | Possibly simplify Closure |

## Success Criteria

1. All existing tests pass
2. No runtime scope chain traversal
3. All variable resolution happens at compile time
4. Single source of truth for environment layout
5. VarRef clearly indicates access method
6. Performance equal or better than current

## Risks and Mitigations

### Risk: Breaking existing code
**Mitigation**: Phased approach, run tests after each phase

### Risk: CPS/bytecode divergence
**Mitigation**: Shared BindingResolver ensures both use same resolution

### Risk: Edge cases in scope resolution  
**Mitigation**: Comprehensive test coverage before starting

### Risk: JIT integration issues
**Mitigation**: JIT already has good scoping; update to use VarRef

## Timeline Estimate

- Phase 1 (Cranelift cleanup): 1 session
- Phase 2-3 (Binding module + AST): 1-2 sessions
- Phase 4-6 (Compiler + VM): 2-3 sessions
- Phase 7-8 (CPS + Converters): 1-2 sessions
- Phase 9 (File splitting): 1 session
- Testing and fixes: 1-2 sessions

Total: ~8-12 sessions
