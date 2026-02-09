# Elle Lisp Interpreter: Comprehensive Scope Handling Analysis Report

## Executive Summary

The Elle Lisp interpreter has a **hybrid scope system** combining:
- **Phase 1 (Compile-time)**: `CompileScope` for variable tracking during parsing
- **Phase 2 (Runtime)**: `ScopeStack` for runtime scope management (partially integrated)

The implementation has several **critical scope violations**, **architectural misalignments**, and **missing implementations** that compromise correctness. This report identifies 21 major issues across 5 categories with severity ratings.

---

## 1. CURRENT SCOPE INFRASTRUCTURE

### 1.1 Compile-Time Scope (`src/compiler/scope.rs`)

**Status**: ✓ Well-implemented for Phase 1

```rust
pub struct CompileScope {
    frames: Vec<ScopeFrame>,
}

pub enum ScopeType {
    Global, Function, Block, Loop, Let,
}

pub enum BindingType {
    Parameter, Local, Captured,
}
```

**Key behaviors**:
- Maintains stack of scopes (frames)
- `lookup()` returns (depth, index) tuple
- `define_local()` adds variable to current scope
- Depth represents "how many scopes up" a variable is defined

**Limitations**:
- Only used for **phase 1 variable resolution** during `value_to_expr`
- Not used in actual **bytecode compilation** (`src/compiler/compile.rs`)
- **Expr::Var(sym, depth, index)** is created during conversion but depth/index are often **placeholder values (0, 0)**

### 1.2 Runtime Scope (`src/compiler/scope.rs`)

**Status**: ⚠ Implemented but unused in bytecode path

```rust
pub struct ScopeStack {
    stack: Vec<RuntimeScope>,
}

pub struct RuntimeScope {
    variables: HashMap<u32, Value>,
    scope_type: ScopeType,
}
```

**Key behaviors**:
- `get(sym_id)` walks up scope chain
- `set(sym_id, value)` updates variable in scope where it was defined
- `define_local()` adds to current scope only
- Thread-safe HashMap-based lookup

**Problems**:
- **NEVER ACTUALLY PUSHED/POPPED** in bytecode execution
- `PushScope/PopScope` instructions exist but are **no-op stubs**
- Runtime code at line 312-333 in `src/vm/mod.rs` handles scope instructions but **compilation never emits them**

### 1.3 Bytecode Compilation (`src/compiler/compile.rs`)

**Status**: ✗ Severely broken - mismatch between phases

**Current variable access patterns**:

1. **LoadGlobal** (line 85-89): Global variable access
   - Used for `Expr::GlobalVar`
   - **Direct globals lookup, no scope checking**

2. **LoadUpvalue** (line 74-83): Closure environment access
   - Used for `Expr::Var` (captured variables)
   - **Hardcoded to use closure environment only**
   - Adds 1 to depth: `emit_byte((*depth + 1) as u8)`

3. **StoreLocal** (line 219): Local variable mutation
   - Used in `Expr::Set` when depth == 0
   - **Directly modifies stack at index** (see line 46 in `src/vm/stack.rs`)

4. **StoreGlobal** (line 233): Global mutation
   - Used for `Expr::Define` and loop variables
   - **All function scope variables treated as globals**

**Critical Issue**: The compilation process **ignores the entire ScopeStack** infrastructure and instead relies on:
- Globals for everything not in a closure
- Stack indices for loop variable access (which is **broken**)

---

## 2. SCOPE VIOLATIONS

### Issue 1: For Loop Variables Are Global, Not Scoped

**Severity**: CRITICAL

**Location**: `src/compiler/compile.rs`, lines 283-348

**Problem**:
```rust
// Line 314-316: Store element in loop variable
let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
self.bytecode.emit(Instruction::StoreGlobal);  // ← WRONG: Should be scoped
self.bytecode.emit_u16(var_idx);
```

**Impact**:
- Loop variable persists **after loop exits** (should be deallocated)
- Multiple nested loops **shadow each other improperly**
- Loop variable accessible **outside intended scope**

**Test Case**:
```scheme
(define result 0)
(for i (list 1 2 3) (set! result (+ result i)))
; i should NOT be accessible here
(if (defined? i) "BUG: i leaked" "OK")
```

**Expected**: Error or nil (i undefined)
**Actual**: i is still defined and has value 3

---

### Issue 2: While Loop Variables Are Global

**Severity**: CRITICAL

**Location**: `src/compiler/compile.rs`, lines 237-281

**Problem**:
```rust
// Line 240-244: Pre-declared variables stored as globals
for sym_id in defines {
    self.bytecode.emit(Instruction::Nil);
    let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
    self.bytecode.emit(Instruction::StoreGlobal);  // ← All globals
    self.bytecode.emit_u16(idx);
}
```

**Impact**:
- Variables defined inside loop bodies become **permanent globals**
- Loop-local state escapes to global namespace
- Same issues as for-loops

**Test Case**:
```scheme
(while (< i 5)
  (define temp (+ i 1))
  (set! i temp))
; temp should NOT exist here
```

---

### Issue 3: LoadUpvalue Works Only in Closures

**Severity**: HIGH

**Location**: `src/vm/variables.rs`, lines 50-73

**Problem**:
```rust
pub fn handle_load_upvalue(
    vm: &mut VM,
    bytecode: &[u8],
    ip: &mut usize,
    closure_env: Option<&std::rc::Rc<Vec<Value>>>,
) -> Result<(), String> {
    // Line 71: ERROR if closure_env is None
    if let Some(env) = closure_env {
        vm.stack.push(env[idx].clone());
    } else {
        return Err("LoadUpvalue used outside of closure".to_string());
    }
}
```

**Impact**:
- **Only works inside lambda execution**
- Cannot access outer scope variables in regular code
- Breaks let-binding implementation entirely

**Code Path**:
```rust
// src/compiler/compile.rs line 74-83
Expr::Var(_sym, depth, index) => {
    self.bytecode.emit(Instruction::LoadUpvalue);
    self.bytecode.emit_byte((*depth + 1) as u8);
    self.bytecode.emit_byte(*index as u8);
}
```

This compiles ALL `Var` expressions (not just in closures) to `LoadUpvalue`, but the VM handler **requires closure environment**.

---

### Issue 4: StoreLocal Uses Raw Stack Index

**Severity**: HIGH

**Location**: `src/vm/stack.rs`, lines 40-47; `src/compiler/compile.rs`, line 219-220

**Problem**:
```rust
// Compilation
Expr::Set { var, depth: 0, index, .. } => {
    self.compile_expr(value, false);
    self.bytecode.emit(Instruction::StoreLocal);
    self.bytecode.emit_byte(*index as u8);
}

// Execution
pub fn handle_store_local(vm: &mut VM, bytecode: &[u8], ip: &mut usize) {
    let idx = vm.read_u8(bytecode, ip) as usize;
    if idx >= vm.stack.len() {
        return Err("Local variable index out of bounds".to_string());
    }
    vm.stack[idx] = val;  // ← Direct stack modification!
}
```

**Impact**:
- **Assumes stack layout matches compile-time expectations**
- **No scope context** - just raw stack indexing
- Works **only for local variables in current function**
- **Broken for nested scopes** (let, blocks, etc.)

**Problem Example**:
```scheme
(define x 1)
(let ((x 2))
  (set! x 3))
; x should be 1 (outer), not 3
```

The `StoreLocal` index doesn't account for:
- Function parameters below locals
- Closure environment values
- Different stack frames

---

### Issue 5: Closure Capture Uses LoadGlobal Pattern

**Severity**: CRITICAL

**Location**: `src/compiler/compile.rs`, lines 183-188

**Problem**:
```rust
// Lambda captures
for (sym, _depth, _index) in captures {
    // Load the captured variable
    let sym_idx = self.bytecode.add_constant(Value::Symbol(*sym));
    self.bytecode.emit(Instruction::LoadGlobal);  // ← Assumes global!
    self.bytecode.emit_u16(sym_idx);
}
```

**Impact**:
- **Can only capture globals**, not parent scope variables
- Closure cannot access variables from **outer function scopes**
- Breaks lexical scoping entirely

**Test Case**:
```scheme
((lambda (outer)
   (lambda (inner)
     (+ outer inner)))
 5)
; Should work: outer=5 captured
```

**Current Behavior**: Tries to load 'outer' from globals (undefined error)

---

### Issue 6: Let Expressions Panic in Compilation

**Severity**: CRITICAL

**Location**: `src/compiler/compile.rs`, lines 196-203

**Problem**:
```rust
Expr::Let { bindings: _, body: _ } => {
    panic!("Unexpected Let expression in compile phase - should have been transformed to lambda call");
}
```

**Impact**:
- **Let expressions should have been converted to lambda calls** by `value_to_expr`
- If they reach compilation, it's an **assertion failure** (panic)
- Indicates **phase boundary violation**

**Why This Happens**:
- `converters.rs` (lines 363-438) **does convert** `let` to `lambda` calls
- But this doesn't happen for **all code paths** (e.g., macro expansion, manual AST construction)

---

### Issue 7: Variable Lookup Ignores Runtime Scope

**Severity**: HIGH

**Location**: `src/vm/mod.rs`, lines 61-87 (execution loop)

**Problem**:
```rust
Instruction::LoadGlobal => {
    variables::handle_load_global(self, bytecode, &mut ip, constants)?;
    // Looks in vm.globals HashMap only!
}

// vs

// src/compiler/converters.rs says variables could use ScopeVar!
Expr::ScopeVar(depth, index) => {
    // This is NEVER compiled to bytecode
    // Compilation just emits LoadUpvalue (line 588-594)
}
```

**Impact**:
- **ScopeStack is never consulted during variable lookups**
- All non-global variable access **fails at runtime**
- The `ScopeVar` expression type exists but is **never used**

---

### Issue 8: Scope Entry/Exit Are No-Ops

**Severity**: HIGH

**Location**: `src/compiler/compile.rs`, lines 597-608

**Problem**:
```rust
Expr::ScopeEntry(scope_type) => {
    let _ = scope_type; // Suppress unused warning
    // NO BYTECODE EMITTED - this is a no-op!
}

Expr::ScopeExit => {
    // NO BYTECODE EMITTED - this is a no-op!
}
```

**Impact**:
- Infrastructure for **PushScope/PopScope** exists in VM
- But **compilation never emits these instructions**
- Runtime ScopeStack is never used

**Expected**: Should emit:
```rust
self.bytecode.emit(Instruction::PushScope);
self.bytecode.emit_byte(scope_type_byte);
// ... scope body ...
self.bytecode.emit(Instruction::PopScope);
```

---

## 3. CLOSURE HANDLING ISSUES

### Issue 9: Closure Environment Is Static, Not Dynamic

**Severity**: HIGH

**Location**: `src/compiler/converters.rs`, lines 341-344; `src/vm/mod.rs`, lines 145-150

**Problem**:
```rust
// Compilation time
let captures: Vec<_> = free_vars
    .iter()
    .map(|sym| (*sym, 0, 0)) // ← Placeholder depth/index (0, 0)
    .collect();

// Execution time
let mut new_env = Vec::new();
new_env.extend((*closure.env).iter().cloned());
new_env.extend(args.clone());
// ← Merges captured values + parameters into one list
```

**Impact**:
- Captured values are **loaded from globals at closure creation time**
- If outer variables **change later, closure sees old values**
- No **closure cell** semantics

**Test Case**:
```scheme
(define x 10)
(define get-x (lambda () x))
(set! x 20)
(get-x)
; Should return 20 (current value)
; Actually returns 10 (stale capture)
```

---

### Issue 10: Free Variable Analysis Incomplete

**Severity**: MEDIUM

**Location**: `src/compiler/analysis.rs`, lines 146-269

**Problem**:
```rust
// analyze_free_vars doesn't properly handle:
Expr::While { cond, body } => {
    // Variables defined in body aren't treated as loop-local
    free_vars.extend(analyze_free_vars(cond, local_bindings));
    free_vars.extend(analyze_free_vars(body, local_bindings));
}

Expr::For { var, iter, body } => {
    // Loop variable IS added to bindings (correct)
    // But captures don't respect loop scope
    let mut new_bindings = local_bindings.clone();
    new_bindings.insert(*var);
    // Still includes outer scope variables
}
```

**Impact**:
- Closures inside loops **incorrectly capture loop variables**
- Closure semantics within loops are **undefined**

---

### Issue 11: Closure Depth/Index Calculation Wrong

**Severity**: HIGH

**Location**: `src/compiler/converters.rs`, lines 333-344; `src/compiler/compile.rs`, lines 74-83

**Problem**:

During **conversion** (`converters.rs`):
```rust
let captures: Vec<_> = free_vars
    .iter()
    .map(|sym| (*sym, 0, 0)) // ← Always (0, 0)
    .collect();
```

During **compilation** (`compile.rs`):
```rust
Expr::Var(_sym, depth, index) => {
    self.bytecode.emit(Instruction::LoadUpvalue);
    self.bytecode.emit_byte((*depth + 1) as u8);  // ← Adjusted depth
    self.bytecode.emit_byte(*index as u8);
}
```

**The depth/index are NEVER properly computed**:
1. `value_to_expr` sets them to (0, 0)
2. Compilation increments depth by 1
3. But the actual position in closure.env is not validated

**Impact**:
- LoadUpvalue accesses **wrong environment index**
- Closure variables are **read/written to wrong slots**

---

## 4. LOOP VARIABLE SCOPING ISSUES

### Issue 12: No Scope Isolation For Loop Bodies

**Severity**: CRITICAL

**Location**: `src/compiler/compile.rs`, lines 237-348

**Problem**:
```rust
// For loop - NO scope frame created
// Line 298-348: Direct manipulation of global state
self.bytecode.emit(Instruction::StoreGlobal);  // var stored as global
// ... loop body ...
self.bytecode.emit(Instruction::Cdr);  // continue with list
```

**Expected behavior**:
```lisp
(for x lst
  (+ x 1))  ; x defined only here
; x not visible here
```

**Actual behavior**:
```lisp
(for x lst
  (+ x 1))
; x still has value of last element!
```

---

### Issue 13: Loop Variable Persistence Across Iterations

**Severity**: HIGH

**Location**: `src/compiler/compile.rs`, lines 313-320

**Problem**:
```rust
// Store element in loop variable (and pop it)
let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
self.bytecode.emit(Instruction::StoreGlobal);
self.bytecode.emit_u16(var_idx);
// var_idx is updated to next element in loop
// But it's a GLOBAL, so it persists
```

**Impact**:
- Loop variable has **stale value after loop exits**
- Can interfere with **subsequent loops**
- No **proper variable cleanup**

---

### Issue 14: Multiple Nested Loops Share Variable Namespace

**Severity**: HIGH

**Location**: `src/compiler/compile.rs`

**Problem**:
```scheme
(for x (list 1 2)
  (for y (list 10 20)
    (+ x y)))

; Both x and y become globals
; If outer loop iterates first, x gets clobbered
```

**No scope isolation means**:
- Inner loop's `y` is global
- Outer loop's `x` is global
- They interfere with each other

---

## 5. LET-BINDING GAPS

### Issue 15: Let Variables Transformed But Not Runtime-Scoped

**Severity**: CRITICAL

**Location**: `src/compiler/converters.rs`, lines 363-438

**Problem**:

`let` is **statically transformed** to lambda:
```scheme
(let ((x 5) (y 10))
  (+ x y))

; Becomes:
((lambda (x y) (+ x y)) 5 10)
```

**BUT at runtime**:
- Lambda parameters are stored in **closure environment**
- Via `LoadUpvalue` accessing `closure.env` vector
- NOT using the `ScopeStack` infrastructure

**Impact**:
- Let-binding works **only if** closure execution path is used
- Regular lambda calls work
- But **no actual scope frame** is created for let-bindings
- Variables aren't in `scope_stack` when needed by other operations

---

### Issue 16: Let Bindings Don't Support Nested Block Scopes

**Severity**: HIGH

**Location**: Systemic - let creates lambda, not runtime scope frame

**Problem**:
```scheme
(let ((x 5))
  (let ((y 10))
    (+ x y)))
```

The inner let becomes another lambda call, which means:
- Each let creates a **new function call**
- No **shared scope frame** across sibling lets
- Variables from outer let **must be captured** (not walking up scope chain)

---

### Issue 17: Let* Sequential Binding Relies on Conversion

**Severity**: MEDIUM

**Location**: `src/compiler/converters.rs`, lines 441-538

**Problem**:
```rust
// let* is expanded at conversion time
// Line 474-496: Variables added to scope during binding parse

scope_stack.push(Vec::new());  // New scope level
for binding in &bindings_vec {
    // Parse binding expression in current scope
    let expr = value_to_expr_with_scope(&binding_list[1], symbols, scope_stack)?;
    // Add variable to scope for next binding
    if let Some(current_scope) = scope_stack.last_mut() {
        current_scope.push(var);
    }
}
```

**This works** but only because:
- Scope tracking is done at **conversion time** (value_to_expr)
- Not at **runtime** via ScopeStack
- Makes **runtime scope introspection impossible**

---

## 6. ARCHITECTURAL MISALIGNMENTS

### Issue 18: Phase Boundary Confusion

**Severity**: HIGH

**The three phases are conflated**:

1. **Phase 1**: Parsing + Conversion (reads Value → writes Expr)
   - Uses: `value_to_expr` with local `scope_stack`
   - Problem: Local scope tracking is **not connected to compiler scope.rs**

2. **Phase 2**: Compilation (reads Expr → writes Bytecode)
   - Uses: `compiler.rs` with no scope tracking
   - Problem: Ignores all CompileScope infrastructure
   - **Never uses CompileScope at all!**

3. **Phase 2 Runtime**: Execution (reads Bytecode + executes)
   - Has: `ScopeStack` infrastructure
   - Problem: Never populated, never used

**Result**: Three separate scope systems that don't talk to each other!

---

### Issue 19: ScopeVar Expression Type Defined But Never Generated

**Severity**: MEDIUM

**Location**: `src/compiler/ast.rs`, lines 145; `src/compiler/compile.rs`, lines 588-594

**Problem**:
```rust
// AST defines it
Expr::ScopeVar(usize, usize),

// But converters never generate it
// They generate Expr::Var instead
Expr::Var(*id, actual_depth, local_index)

// And compilation ignores it
Expr::ScopeVar(depth, index) => {
    self.bytecode.emit(Instruction::LoadUpvalue);  // ← WRONG type
}
```

**Should be**:
```rust
Expr::ScopeVar(depth, index) => {
    self.bytecode.emit(Instruction::LoadScoped);
    self.bytecode.emit_byte(*depth as u8);
    self.bytecode.emit_byte(*index as u8);
}
```

---

### Issue 20: ScopeEntry/ScopeExit Never Emitted

**Severity**: HIGH

**Location**: `src/compiler/ast.rs`, lines 149-153; `src/compiler/compile.rs`, lines 597-608

**Problem**:
```rust
// Expression types exist
Expr::ScopeEntry(ScopeType),
Expr::ScopeExit,

// But never generated during compilation
// And compilation doesn't emit their instructions
```

**Should be generated for**:
- Each `let` binding scope
- Each block scope
- Each loop scope

---

## 7. INSTRUCTION HANDLER GAPS

### Issue 21: LoadScoped Is Implemented But Never Called

**Severity**: MEDIUM

**Location**: `src/vm/scope.rs`, lines 220-232

**Problem**:
```rust
pub fn handle_load_scoped(_vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = bytecode[*ip] as usize;
    *ip += 1;
    let index = bytecode[*ip] as usize;
    *ip += 1;

    // This instruction is for future use - currently variables use LoadUpvalue
    // For now, just treat as a no-op to avoid breaking existing code
    let _ = depth;
    let _ = index;
    Ok(())  // ← NO-OP!
}
```

**Purpose**: Should load variable from scope at specific depth/index
**Current behavior**: No-op stub
**Never called**: Because `Expr::ScopeVar` never compiled to `LoadScoped`

---

## SUMMARY TABLE

| Issue | Category | Severity | File | Lines | Status |
|-------|----------|----------|------|-------|--------|
| 1 | Loop Variables | CRITICAL | compile.rs | 314-316 | Broken |
| 2 | While Loop Variables | CRITICAL | compile.rs | 237-244 | Broken |
| 3 | LoadUpvalue Only in Closures | HIGH | variables.rs | 50-73 | Broken |
| 4 | StoreLocal Raw Index | HIGH | stack.rs | 40-47 | Broken |
| 5 | Closure Capture Global-Only | CRITICAL | compile.rs | 183-188 | Broken |
| 6 | Let Expression Panic | CRITICAL | compile.rs | 196-203 | Broken |
| 7 | Ignores Runtime Scope | HIGH | mod.rs | 61-87 | Broken |
| 8 | ScopeEntry/Exit No-Op | HIGH | compile.rs | 597-608 | Broken |
| 9 | Static Closure Environment | HIGH | converters.rs | 341-344 | Broken |
| 10 | Free Variable Analysis | MEDIUM | analysis.rs | 146-269 | Incomplete |
| 11 | Closure Depth Calculation | HIGH | converters.rs | 333-344 | Broken |
| 12 | No Loop Body Isolation | CRITICAL | compile.rs | 237-348 | Broken |
| 13 | Loop Variable Persistence | HIGH | compile.rs | 313-320 | Broken |
| 14 | Nested Loops Share Namespace | HIGH | compile.rs | N/A | Broken |
| 15 | Let Not Runtime-Scoped | CRITICAL | converters.rs | 363-438 | Broken |
| 16 | Let Nested Block Scopes | HIGH | Systemic | N/A | Broken |
| 17 | Let* Sequential | MEDIUM | converters.rs | 441-538 | Works but fragile |
| 18 | Phase Boundary Confusion | HIGH | Systemic | N/A | Architectural |
| 19 | ScopeVar Never Generated | MEDIUM | compile.rs | 145, 588 | Dead code |
| 20 | ScopeEntry Never Emitted | HIGH | compile.rs | 149-153 | Dead code |
| 21 | LoadScoped Is No-Op | MEDIUM | scope.rs | 220-232 | Stub |

---

## RECOMMENDED FIXES (Priority Order)

### CRITICAL (Phase 2 Redesign Required):

1. **Emit PushScope/PopScope for all scoped constructs** (loops, blocks, let)
2. **Implement proper LoadScoped/StoreScoped** for non-closure variable access
3. **Generate ScopeEntry/ScopeExit** expressions in compilation
4. **Fix closure capture** to support capturing from any scope level
5. **Stop treating loop/let variables as globals**

### HIGH (Medium refactor):

6. Implement proper **StoreScoped** instruction
7. **Remove** LoadUpvalue path for non-closure code
8. Generate correct **depth/index** during conversion
9. Integrate **CompileScope** into actual compilation pipeline
10. Create **scope frames** at runtime for all scoped constructs

### MEDIUM (Cleanup):

11. Remove **ScopeVar/ScopeEntry dead code** or implement it
12. Implement actual **LoadScoped handler** (not no-op)
13. Fix **free variable analysis** for loop scopes
14. Add **proper closure cell semantics** for mutable captures

---

## CONCLUSION

The Elle Lisp interpreter has a **fundamental architectural mismatch** between its compile-time scope infrastructure and runtime scope infrastructure. The system attempts to use:

- **Globals** for all non-closure variables
- **Closure environments** for lambda captures
- **Stack indices** for local mutation
- A completely **unused ScopeStack** at runtime

This results in **scoping violations** where loop variables, let-bindings, and nested scopes don't properly isolate variables. A complete **Phase 2 implementation** is needed to route all variable access (not just closures) through the proper scope stack infrastructure.

The good news: All necessary infrastructure pieces exist (CompileScope, ScopeStack, instructions). They just need to be **properly integrated**.

