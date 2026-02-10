# Architectural Issue: Phases 2/3 Break Local Variable Scoping

## Problem

Phases 2 and 3 attempted to fix `set!` in lambda bodies by using `StoreGlobal` for all defines, including those inside lambdas. This breaks local variable semantics:

1. Local defines inside lambdas use `StoreGlobal` instead of `DefineLocal`
2. This causes them to be stored in the runtime scope_stack
3. Later references to those variables are compiled as `GlobalVar` (LoadGlobal) because the compiler doesn't know they were defined
4. LoadGlobal fails at runtime because the scope_stack lookup fails

## Root Cause

In `src/compiler/converters.rs` line 554-555:
```
// The only variables in the compile-time scope_stack should be lambda parameters
// and captures, which are fixed at compile time.
```

This design prevents tracking of variables defined inside lambda bodies, making them inaccessible at runtime.

## Correct Solution

1. **Use DefineLocal for local lambda defines** - Not StoreGlobal
2. **Track defined locals at compile time** - Update scope_stack after each define so subsequent references know they're local
3. **Use Var for local references** - Not GlobalVar, which requires symbol resolution
4. **Apply cells for shared mutations** - Only wrap in cells if the variable is captured by multiple closures and mutated

## What Phase 4 Should Do

Phase 4 (shared mutable captures) should:
- Properly implement cell boxing for variables that are:
  - Defined locally in a lambda
  - Captured by nested closures  
  - Mutated by those nested closures
- Transparently wrap/unwrap cells on load/store

But this can only work if Phases 1-3 are fundamentally restructured to:
- Use DefineLocal for local defines (not StoreGlobal)
- Track locals at compile time (update scope_stack)
- Generate Var/LoadLocal code for local references (not GlobalVar/LoadGlobal)

## Impact

Currently:
- Any lambda with local define and set! fails at runtime
- Even without set!, local defines fail if their value is referenced
- Unit tests pass because they may use a different execution path
- Examples cannot be tested

This is a blocker for Phase 4 and requires significant compiler refactoring.
