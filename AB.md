# Parallel Development Plan: Issues #19 & #20

## Overview
Two non-overlapping work tracks for scope/closure/recursion improvements:
- **Track A**: Issue #19 - Mutual Recursion Support
- **Track B**: Issue #20 - Closure Optimization

## Track A: Mutual Recursion (Issue #19)

### Current State
- Simple recursion works (Issue #18 completed)
- Single function can reference itself
- Problem: Functions cannot reference each other (forward references fail)

### Architecture
Works at: **Compiler & Begin block level**
- Converter: Parse function definitions
- Compile: Handle forward references
- Begin: Pre-declare all function names

### Implementation Strategy
**Approach: Two-pass compilation at Begin level**

1. **First Pass**: Scan all Begin-level defines, create placeholders
   - Pre-declare all function names as `nil` in globals
   - Create dependency graph
   
2. **Second Pass**: Compile function bodies
   - All function names now available for reference
   - Functions can call each other
   - Detection of circular references (warning, not error)

3. **Key Changes Needed**
   - Enhance `collect_defines()` in compile.rs
   - Track function dependencies
   - Already has pre-declaration mechanism (from Issue #18)

### File Changes
- `src/compiler/compile.rs` - Enhance Begin block handling
- `src/compiler/converters.rs` - Track dependencies
- `tests/integration/core.rs` - Add mutual recursion tests

### Test Cases
```lisp
(define is-even (lambda (n) (if (= n 0) #t (is-odd (- n 1)))))
(define is-odd (lambda (n) (if (= n 0) #f (is-even (- n 1)))))
(is-even 4)  ; => #t
```

### Isolation
- ✓ Only affects Begin block & define compilation
- ✓ Doesn't touch closure environment or VM
- ✓ Non-invasive to existing closures

---

## Track B: Closure Optimization (Issue #20)

### Current State
- Closures work correctly
- No optimizations for capture efficiency
- Large environments waste memory
- No dead code elimination

### Architecture
Works at: **VM & Closure environment level**
- VM: LoadUpvalue instruction execution
- Value: Closure structure
- Analysis: Free variable detection

### Implementation Strategy
**Phased approach:**

1. **Phase 1: Foundation - Capture Usage Analysis**
   - Add analysis function to detect if variables are used in lambda bodies
   - Handle simple non-nested cases first
   - Identify which free variables are actually referenced

2. **Phase 2: Dead Capture Elimination (Simple Lambdas)**
   - Only apply optimization to leaf lambdas (no nested lambdas inside)
   - Filter out unused captures
   - Update LoadUpvalue indices for remaining captures
   - Maintain backward compatibility

3. **Phase 3: Nested Lambda Support**
   - Extend to handle closures with nested lambdas
   - Propagate capture requirements upward
   - Ensure nested lambdas can access parent captures

4. **Phase 4: Capture Reordering (Optional)**
   - Group frequently accessed captures
   - Improve CPU cache locality
   - Order by usage frequency

5. **Phase 5: Upvalue Caching (Optional)**
   - Cache frequently accessed upvalues
   - Inline simple upvalue access

### File Changes
- `src/compiler/analysis.rs` - Track capture usage
- `src/compiler/compile.rs` - Filter unused captures
- `tests/integration/closures_and_lambdas.rs` - Benchmark tests

### Performance Metrics
- Measure capture environment size before/after
- Benchmark deep nesting (nested lambdas)
- Test large capture sets

### Isolation
- ✓ Analysis happens at compilation time
- ✓ Only affects closure creation
- ✓ Doesn't change function semantics
- ✓ Backward compatible

---

## Why These Don't Conflict

| Aspect | Track A | Track B |
|--------|---------|---------|
| **Compiler Level** | Begin block pre-declaration | Free variable analysis |
| **VM Level** | Function calling mechanism | Closure environment optimization |
| **Files Modified** | converters.rs, compile.rs | analysis.rs, compile.rs |
| **Critical Section** | Define/call handling | Closure creation |
| **Dependencies** | None on existing closures | None on recursion |

The only shared file is `compile.rs`, but they modify different functions:
- Track A: `compile_expr()` at Begin block
- Track B: `compile_expr()` at Lambda with capture filtering

---

## Success Criteria

### Track A (Mutual Recursion)
- [ ] Even/odd mutual recursion works
- [ ] Three-way recursion works (f → g → h → f)
- [ ] All 807 tests still pass
- [ ] No performance regression

### Track B (Closure Optimization)
- [ ] Dead captures eliminated (size reduction)
- [ ] Environment lookup remains O(1)
- [ ] All 807+ tests pass
- [ ] 10-20% memory reduction on closure-heavy code

---

## Timeline Estimate

- **Track A Setup**: 2-3 hours
- **Track A Implementation**: 4-6 hours
- **Track A Testing**: 2-3 hours
- **Total Track A**: 8-12 hours

- **Track B Setup**: 2-3 hours
- **Track B Implementation**: 4-6 hours
- **Track B Testing/Benchmarking**: 3-4 hours
- **Total Track B**: 9-13 hours

**Parallel Duration**: ~12-13 hours wall clock time
