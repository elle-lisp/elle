# Elle Scope Implementation Roadmap

## Implementation Phases

### Phase 2.1: Scope Stack Integration (CRITICAL - Must do first)

**Goal**: Make ScopeStack actually used at runtime

#### 2.1.1 Emit PushScope/PopScope Instructions

**Files to modify**:
- `src/compiler/compile.rs` - Add scope frame emission

**Changes needed**:
```rust
// For While loops (line 237-281)
// BEFORE: Pre-declare all defines as globals
// AFTER:
// 1. Emit: PushScope(Loop)
// 2. Compile defines locally (DefineLocal, not StoreGlobal)
// 3. Compile body
// 4. Emit: PopScope

// For For loops (line 283-348)
// BEFORE: StoreGlobal for loop variable
// AFTER:
// 1. Emit: PushScope(Loop)
// 2. Emit: DefineLocal(var)
// 3. Compile body
// 4. Emit: PopScope

// For Let expressions (line 196-203)
// BEFORE: Panic
// AFTER:
// 1. Emit: PushScope(Let)
// 2. Compile binding expressions
// 3. Emit: DefineLocal for each binding
// 4. Compile body
// 5. Emit: PopScope
```

**Tests to pass**:
- Loop variable doesn't persist after loop
- Nested loops don't interfere
- Let variables are properly scoped
- Block scopes work

**Estimated effort**: 4 hours

---

#### 2.1.2 Implement DefineLocal at Runtime

**Files to modify**:
- `src/vm/scope.rs` - Enhance handle_define_local
- `src/vm/mod.rs` - Call DefineLocal handler

**Current behavior** (line 256-282 in scope.rs):
```rust
pub fn handle_define_local(...) -> Result<(), String> {
    // Reads symbol from constants
    // Gets symbol ID
    // vm.scope_stack.define_local(sym_id, value)
    // OK!
}
```

**Problem**: This works but **when is it called**?
- Never! `Expr::DefineLocal` never compiled

**Changes needed**:
```rust
// In compile.rs, for all variable definitions in scopes:
// Expr::Define inside scope frame:
//   → emit DefineLocal (not StoreGlobal)
```

**Tests to pass**:
- Variables defined in scopes are findable via ScopeStack
- They disappear when scope exits
- Shadowing works correctly

**Estimated effort**: 2 hours

---

### Phase 2.2: Fix Variable Access Instructions

**Goal**: Route ALL variable access through ScopeStack (except globals)

#### 2.2.1 Implement LoadScoped Instruction

**Files to modify**:
- `src/vm/scope.rs` - Implement handle_load_scoped (currently no-op)

**Current code** (line 220-232):
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
    Ok(())  // ← STUB
}
```

**Fix**:
```rust
pub fn handle_load_scoped(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = vm.read_u8(bytecode, ip) as usize;
    let index = vm.read_u8(bytecode, ip) as usize;
    
    // Get variable at specific depth in scope stack
    // Scope stack looks like: [Global, ..., Current]
    // depth=0 means current, depth=1 means parent, etc.
    
    if let Some(value) = vm.scope_stack.get_at_depth(depth, index as u32) {
        vm.stack.push(value);
        Ok(())
    } else {
        Err(format!("Variable at depth {} index {} not found", depth, index))
    }
}
```

**Wait - Problem**: ScopeStack uses `HashMap<u32, Value>` (symbol_id → Value)
But we need index-based access for bytecode instructions!

**Solution**: Change ScopeStack to use:
```rust
pub struct RuntimeScope {
    pub variables: Vec<Value>,  // Index-based, not HashMap!
    pub variable_names: HashMap<u32, usize>,  // symbol_id → index (for debugging)
    pub scope_type: ScopeType,
}
```

**Tests to pass**:
- Can load variable from current scope via depth=0, index
- Can load from parent scope via depth=1, index
- Proper error on invalid depth/index

**Estimated effort**: 3 hours (includes ScopeStack refactor)

---

#### 2.2.2 Implement StoreScoped Instruction

**Files to modify**:
- `src/vm/scope.rs` - Implement handle_store_scoped

**Current code** (line 234-253):
```rust
pub fn handle_store_scoped(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = bytecode[*ip] as usize;
    *ip += 1;
    let index = bytecode[*ip] as usize;
    *ip += 1;

    let value = vm.stack.pop().ok_or("Stack underflow")?;

    if !vm.scope_stack.set_at_depth(depth, index as u32, value) {
        return Err(format!("Variable not found at depth {} index {}", depth, index));
    }

    Ok(())
}
```

**Problem**: Uses symbol ID as key (u32), should use index

**Fix**:
```rust
pub fn handle_store_scoped(vm: &mut VM, bytecode: &[u8], ip: &mut usize) -> Result<(), String> {
    let depth = vm.read_u8(bytecode, ip) as usize;
    let index = vm.read_u8(bytecode, ip) as usize;
    let value = vm.stack.pop().ok_or("Stack underflow")?;

    if let Some(scope) = vm.scope_stack.scope_mut_at_depth(depth) {
        if index < scope.variables.len() {
            scope.variables[index] = value;
            Ok(())
        } else {
            Err(format!("Variable index {} out of bounds", index))
        }
    } else {
        Err(format!("Scope depth {} not found", depth))
    }
}
```

**Tests to pass**:
- Can store variable to current scope
- Can store to parent scope
- Value properly updated when retrieved later

**Estimated effort**: 2 hours

---

#### 2.2.3 Stop Using LoadUpvalue For Non-Closures

**Files to modify**:
- `src/compiler/compile.rs` - Change how Expr::Var is compiled

**Current code** (line 74-83):
```rust
Expr::Var(_sym, depth, index) => {
    // Variables in closure environment - access via LoadUpvalue
    self.bytecode.emit(Instruction::LoadUpvalue);
    self.bytecode.emit_byte((*depth + 1) as u8);
    self.bytecode.emit_byte(*index as u8);
}
```

**Problem**: 
- Used for ALL Expr::Var, not just closures
- LoadUpvalue requires closure_env to be present
- LoadUpvalue adds 1 to depth (why?)

**Fix**:
```rust
Expr::Var(_sym, depth, index) => {
    // Check if we're in a closure context somehow?
    // For now, use LoadScoped for all Var references
    self.bytecode.emit(Instruction::LoadScoped);
    self.bytecode.emit_byte(*depth as u8);
    self.bytecode.emit_byte(*index as u8);
}
```

**Tests to pass**:
- Closure variables still work
- Scoped variables work
- Let-bound variables work

**Estimated effort**: 2 hours

---

### Phase 2.3: Fix Loop Variable Scoping

**Goal**: Loop variables properly scoped, not globals

#### 2.3.1 Scope For Loops

**Files to modify**:
- `src/compiler/compile.rs` - lines 283-348

**Current broken code**:
```rust
// Pre-declare defines in loop body as globals (line 284-291)
for sym_id in defines {
    self.bytecode.emit(Instruction::Nil);
    let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
    self.bytecode.emit(Instruction::StoreGlobal);  // ← WRONG
    self.bytecode.emit_u16(idx);
}

// Store loop variable as global (line 314-316)
let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
self.bytecode.emit(Instruction::StoreGlobal);  // ← WRONG
self.bytecode.emit_u16(var_idx);
```

**Proposed fix**:
```rust
// Push loop scope
self.bytecode.emit(Instruction::PushScope);
self.bytecode.emit_byte(ScopeType::Loop as u8);

// Define loop variable in scope
self.bytecode.emit(Instruction::Nil);  // Dummy initial value
let var_idx = self.bytecode.add_constant(Value::Symbol(*var));
self.bytecode.emit(Instruction::DefineLocal);
self.bytecode.emit_u16(var_idx);

// Pre-declare defines in loop body as locals in loop scope
for sym_id in defines {
    self.bytecode.emit(Instruction::Nil);
    let idx = self.bytecode.add_constant(Value::Symbol(sym_id));
    self.bytecode.emit(Instruction::DefineLocal);
    self.bytecode.emit_u16(idx);
}

// Loop iterations (Dup, Cdr, etc.) - unchanged
// But now use StoreScoped for loop var instead of StoreGlobal
self.bytecode.emit(Instruction::StoreScoped);
self.bytecode.emit_byte(0);  // Current scope
self.bytecode.emit_byte(0);  // First variable (the loop var)

// Pop loop scope
self.bytecode.emit(Instruction::PopScope);
```

**Problem**: Need to track variable indices properly in loop scope

**Tests to pass**:
- Loop variable doesn't exist after loop
- Multiple loops don't interfere
- Nested loops work correctly
- Loop variables shadow outer variables properly

**Estimated effort**: 4 hours

---

#### 2.3.2 Scope While Loops

**Similar to 2.3.1 but for while loops**

**Files to modify**:
- `src/compiler/compile.rs` - lines 237-281

**Tests to pass**:
- Variables defined in while loop body are local to loop
- Loop variables don't persist

**Estimated effort**: 3 hours

---

### Phase 2.4: Fix Closure Captures

**Goal**: Closures can capture from ANY scope, not just globals

#### 2.4.1 Implement Proper Capture Resolution

**Files to modify**:
- `src/compiler/converters.rs` - lines 333-344
- `src/compiler/compile.rs` - lines 183-188

**Current broken behavior**:
```rust
// converters.rs - All captures get (0, 0)
let captures: Vec<_> = free_vars
    .iter()
    .map(|sym| (*sym, 0, 0)) // ← WRONG
    .collect();

// compile.rs - All loads assume global
for (sym, _depth, _index) in captures {
    let sym_idx = self.bytecode.add_constant(Value::Symbol(*sym));
    self.bytecode.emit(Instruction::LoadGlobal);  // ← WRONG
    self.bytecode.emit_u16(sym_idx);
}
```

**Fix requires**:
1. Track actual depth/index during free variable analysis
2. Load from appropriate scope (not just global) during closure creation
3. Store depth/index in closure environment somehow

**Challenge**: Closure environment is just `Vec<Value>`, no metadata
Need to change Closure structure to include capture info:

```rust
pub struct Closure {
    pub bytecode: Rc<Vec<u8>>,
    pub arity: Arity,
    pub env: Rc<Vec<Value>>,
    pub env_depths: Rc<Vec<(usize, usize)>>,  // NEW: (depth, index) for each capture
    pub num_locals: usize,
    pub constants: Rc<Vec<Value>>,
}
```

**Tests to pass**:
- Closures capture from parent functions
- Nested closures work
- Captures are fresh (not stale)

**Estimated effort**: 6 hours

---

### Phase 2.5: Fix Set! (Variable Mutation)

**Goal**: set! works for ANY scoped variable, not just locals and globals

#### 2.5.1 Implement StoreUpvalue

**Files to modify**:
- `src/compiler/bytecode.rs` - Add StoreUpvalue instruction
- `src/vm/mod.rs` - Add dispatch case
- `src/vm/variables.rs` - Implement handler

**Current broken code** (compile.rs:217-227):
```rust
Expr::Set { var, depth, index, value } => {
    self.compile_expr(value, false);
    if *index == usize::MAX {
        // Global variable set
        let idx = self.bytecode.add_constant(Value::Symbol(*var));
        self.bytecode.emit(Instruction::StoreGlobal);
        self.bytecode.emit_u16(idx);
    } else if *depth == 0 {
        // Local variable set
        self.bytecode.emit(Instruction::StoreLocal);
        self.bytecode.emit_byte(*index as u8);
    } else {
        // Upvalue variable set (not supported yet - treat as error or global)
        // For now, treat as global to avoid corruption
        let idx = self.bytecode.add_constant(Value::Symbol(*var));
        self.bytecode.emit(Instruction::StoreGlobal);  // ← WRONG
        self.bytecode.emit_u16(idx);
    }
}
```

**Fix**:
```rust
Expr::Set { var, depth, index, value } => {
    self.compile_expr(value, false);
    if *index == usize::MAX {
        // Global variable set
        self.bytecode.emit(Instruction::StoreGlobal);
    } else if *depth == 0 {
        // Local variable set (in current scope or closure)
        if is_in_closure {
            self.bytecode.emit(Instruction::StoreUpvalue);
            self.bytecode.emit_byte(0);  // Current closure level
            self.bytecode.emit_byte(*index as u8);
        } else {
            self.bytecode.emit(Instruction::StoreLocal);
            self.bytecode.emit_byte(*index as u8);
        }
    } else {
        // Upvalue variable set
        self.bytecode.emit(Instruction::StoreUpvalue);
        self.bytecode.emit_byte(*depth as u8);
        self.bytecode.emit_byte(*index as u8);
    }
}
```

**Problem**: Need way to know if we're inside closure during compilation

**Tests to pass**:
- set! works on outer scope variables
- set! works in closures
- Proper errors for undefined variables

**Estimated effort**: 4 hours

---

## Implementation Priority

### Must Do First (Blockers)
1. **2.1.1** Emit PushScope/PopScope - Foundation for everything
2. **2.1.2** Implement DefineLocal at runtime - Part of foundation
3. **2.2.1** Implement LoadScoped - Basic variable access
4. **2.2.2** Implement StoreScoped - Basic variable mutation

### High Priority (Fixes major bugs)
5. **2.3.1** Scope For Loops - Fixes loop variable persistence
6. **2.3.2** Scope While Loops - Fixes loop variable persistence
7. **2.2.3** Stop using LoadUpvalue for non-closures - Fixes crashes

### Medium Priority (Completeness)
8. **2.4.1** Proper Closure Captures - Fixes closure scope issues
9. **2.5.1** Implement StoreUpvalue - Fixes set! in scopes

## Implementation Checklist

### Phase 2.1
- [ ] Modify compile_expr to track scope context
- [ ] Emit PushScope for while loops
- [ ] Emit PopScope for while loops
- [ ] Emit PushScope for for loops
- [ ] Emit PopScope for for loops
- [ ] Emit PushScope for let bindings
- [ ] Emit PopScope for let bindings
- [ ] Test loop variable isolation

### Phase 2.2
- [ ] Refactor ScopeStack to use Vec instead of HashMap
- [ ] Implement handle_load_scoped fully
- [ ] Implement handle_store_scoped fully
- [ ] Update compile_expr to emit LoadScoped for Var
- [ ] Test variable access in scopes

### Phase 2.3
- [ ] Update For loop compilation
- [ ] Update While loop compilation
- [ ] Test for loop scoping
- [ ] Test while loop scoping
- [ ] Test nested loops

### Phase 2.4
- [ ] Track capture depths during analysis
- [ ] Emit proper loads for captures
- [ ] Update Closure structure with env_depths
- [ ] Test closure captures from functions

### Phase 2.5
- [ ] Add StoreUpvalue instruction
- [ ] Implement StoreUpvalue handler
- [ ] Update Set! compilation
- [ ] Test set! on various scopes

## Risk Analysis

### High Risk
- **ScopeStack refactoring** (Vec vs HashMap) - Could break existing tests
  - Mitigation: Keep HashMap path, add Vec-based lookup
- **Closure environment changes** - Requires Closure struct modification
  - Mitigation: Make env_depths optional

### Medium Risk
- Instruction order and bytecode assumptions
  - Mitigation: Careful testing after each phase
- Scope type encoding in bytecode
  - Mitigation: Use existing ScopeType enum

### Low Risk
- New instructions (LoadScoped, StoreScoped already defined)
- Handler implementations (mostly copy-paste from existing)

## Testing Strategy

### Unit Tests to Add
1. For loop variables don't persist
2. While loop variables don't persist
3. Multiple nested loops work
4. Let bindings create proper scope
5. Set! works on parent scopes
6. Closures capture correctly
7. ScopeStack operations work

### Integration Tests to Update
- All loop tests (should be identical but now correct)
- All closure tests
- All let/let* tests

### Regression Tests
- Run all existing tests after each phase
- Check that globals still work
- Verify no new panics

## Estimated Total Effort

- Phase 2.1: 6 hours
- Phase 2.2: 5 hours
- Phase 2.3: 7 hours
- Phase 2.4: 6 hours
- Phase 2.5: 4 hours
- Testing & debugging: 10 hours
- Documentation: 3 hours

**Total: ~41 hours** (5-6 days of focused work)

