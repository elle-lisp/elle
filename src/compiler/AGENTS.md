# compiler

Bytecode compilation, JIT coordination, and supporting infrastructure.
This is a large module; prefer the new pipeline (`hir/` → `lir/`) for
new development.

## Responsibility

- Bytecode instruction definitions
- Legacy AST-based compilation (being replaced)
- CPS transformation (alternative execution path)
- Cranelift JIT compilation
- Effect inference
- Macro expansion support
- Linting

## Submodules

| Module | Purpose |
|--------|---------|
| `bytecode.rs` | `Instruction` enum, `Bytecode` struct |
| `compile/` | Legacy `Expr` → Bytecode compilation |
| `ast.rs` | Legacy `Expr` AST type |
| `converters/` | `Value` ↔ `Expr` conversion |
| `cps/` | Continuation-passing style transform and interpreter |
| `cranelift/` | Native code generation via Cranelift |
| `effects/` | `Effect` enum, inference |
| `linter/` | Static analysis |
| `scope.rs` | Legacy scope tracking |
| `capture_resolution.rs` | Legacy capture analysis |
| `jit_coordinator.rs` | Hot path detection, JIT triggering |
| `jit_executor.rs` | Native code execution |
| `jit_wrapper.rs` | `compile_jit`, `is_jit_compilable` |

## Two pipelines

**New (preferred)**:
```
Syntax → HIR → LIR → Bytecode
```
Located in `hir/`, `lir/`, `pipeline.rs`. Uses `BindingId`.

**Legacy**:
```
Value → Expr → Bytecode
```
Located here. Uses `VarRef`. Being phased out.

## Dependents

- `pipeline.rs` - uses `Bytecode`
- `vm/` - executes bytecode, calls JIT code
- `primitives/jit.rs` - exposes JIT to Elle code

## Invariants

1. **`Instruction` byte values are stable.** Changing them breaks existing
   bytecode. Add new instructions at the end.

2. **Effect inference is conservative.** Unknown calls are `IO`. Only proven
   pure code is `Pure`.

3. **JIT compilation is optional.** Must always have bytecode fallback. A
   `JitClosure` with null code_ptr uses source closure.

4. **CPS is an alternative, not a replacement.** Some primitives like
   `coroutine-resume` use CPS for yield semantics.

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Instruction` | `bytecode.rs` | Bytecode opcodes |
| `Bytecode` | `bytecode.rs` | Instructions + constants |
| `Expr` | `ast.rs` | Legacy AST |
| `Effect` | `effects/mod.rs` | `Pure`, `IO`, `Divergent`, `Yields` |
| `Continuation` | `cps/mod.rs` | CPS continuation |
| `JitCoordinator` | `jit_coordinator.rs` | Hot path tracking |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 32 | Re-exports |
| `bytecode.rs` | ~200 | Instruction enum, Bytecode struct |
| `ast.rs` | ~300 | Legacy Expr type |
| `compile/mod.rs` | ~800 | Legacy compilation |
| `effects/mod.rs` | ~50 | Effect type |
| `effects/inference.rs` | ~300 | Effect inference |
| `cps/` | ~1500 | CPS transform and interpreter |
| `cranelift/` | ~500 | Cranelift code generation |

## Anti-patterns

- Adding features to legacy `compile/` instead of `hir/`+`lir/`
- Modifying `Instruction` byte values
- Assuming JIT is available (always check `is_jit_compilable`)
