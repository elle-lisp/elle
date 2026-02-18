# CPS Rework: Unified Continuation-Based Execution

This document outlines the plan to unify Elle's execution model around
first-class continuations in bytecode/LIR, eliminating the separate CPS
interpreter path.

## Current State (Post-Phase 3)

Elle now has a **single execution path** for all coroutines:

1. **Bytecode VM only**: All code, yielding or not, executes via bytecode
2. **First-class continuations**: `Value::Continuation` holds a chain of
   `ContinuationFrame`s, each capturing bytecode, constants, environment,
   IP, stack, and exception handler state
3. **Frame chain mechanism**: When yield propagates through call boundaries,
   each caller's frame is appended to the continuation chain
4. **Exception handler preservation**: `handler-case` blocks active at yield
   time remain active after resume

The CPS interpreter has been deleted (~4,400 lines removed). The `source_ast`
field remains on `Closure` only for JIT compilation, which still uses the
old `Expr` AST.

### Key implementation details

- `ContinuationFrame` stores: bytecode, constants, env, ip, stack,
  exception_handlers, handling_exception
- Frame ordering: innermost (yielder) first, outermost (caller) last
- `append_frame` is O(1) (was O(n) with `prepend_frame`)
- `resume_continuation` iterates frames forward, restoring handler state
- Exception check at start of instruction loop handles cross-frame propagation
- Tail calls handled in `execute_bytecode_from_ip_with_state`

## Target State (Future)

The remaining work is JIT support for yields:

1. **LIR continuation instructions**: Yield as a terminator, explicit
   continuation capture/apply
2. **JIT consumes LIR**: Rewrite Cranelift codegen to use LIR instead of Expr
3. **Compile Yield in Cranelift**: Generate native code for yield points

## Phased Implementation

### Phase 0: Prerequisites

**Goal**: Ensure the foundation is solid before restructuring.

#### Step 0.1: Merge NaN-boxing Value rework

The `value-nan-boxing` branch must land first. The new `Value` representation
affects continuation capture and environment handling.

**Verification**: `cargo test` passes on main with new Value types.

#### Step 0.2: Audit current continuation usage

Document every place that creates, stores, or inspects continuations:

- `compiler/cps/continuation.rs` - `Continuation` enum
- `compiler/cps/interpreter.rs` - `CpsInterpreter`
- `compiler/cps/trampoline.rs` - `Trampoline`, `TrampolineResult`
- `primitives/coroutines.rs` - `execute_coroutine_cps`, `resume_coroutine_cps`
- `value.rs` - `Coroutine::saved_continuation`

**Deliverable**: Comment in each location marking it for Phase 2 removal or
Phase 1 modification.

#### Step 0.3: Add integration tests for coroutine edge cases

Before changing anything, ensure test coverage for:

- Yield in nested function calls
- Yield in loops (while, for)
- Yield with mutable captures
- Multiple coroutines interleaved
- Recursive yielding functions
- Coroutine that never yields (pure function wrapped as coroutine)

**Deliverable**: `tests/coroutines_comprehensive.rs` with all edge cases.

---

### Phase 1: Continuation as Value

**Goal**: Make continuations first-class values that the VM can manipulate.

#### Step 1.1: Define `Value::Continuation`

Add a new variant to `Value`:

```rust
pub enum Value {
    // ... existing variants ...
    
    /// First-class continuation
    Continuation(Rc<ContinuationData>),
}

/// Data for a captured continuation
pub struct ContinuationData {
    /// Bytecode to resume (shared with closure)
    pub bytecode: Rc<Vec<u8>>,
    /// Constants for the bytecode
    pub constants: Rc<Vec<Value>>,
    /// Instruction pointer to resume at
    pub resume_ip: usize,
    /// Captured environment (locals + captures)
    pub env: Rc<Vec<Value>>,
    /// Operand stack at capture point
    pub operand_stack: Vec<Value>,
    /// Number of locals in the frame
    pub num_locals: usize,
}
```

**Note**: With NaN-boxing, `Rc<ContinuationData>` will be a tagged pointer.

**Verification**: `Value::Continuation` can be created and pattern-matched.

#### Step 1.2: Add bytecode instructions for continuations

Add to `Instruction` enum in `bytecode.rs`:

```rust
/// Capture current continuation as a value
/// Stack: [] -> [continuation]
CaptureCont = 170,

/// Apply a continuation with a value (does not return)
/// Stack: [continuation, value] -> (transfers control)
ApplyCont = 171,

/// Yield with continuation (returns to trampoline)
/// Stack: [value] -> (returns Action::Yield)
YieldCont = 172,
```

**Note**: `YieldCont` is distinct from the existing `Yield` instruction. The
old `Yield` saves a `SavedContext`; the new `YieldCont` captures a proper
continuation value.

**Verification**: Instructions can be encoded/decoded; disassembler shows them.

#### Step 1.3: Implement `CaptureCont` in VM

In `vm/mod.rs`, handle the new instruction:

```rust
Instruction::CaptureCont => {
    let cont_data = ContinuationData {
        bytecode: current_bytecode.clone(),
        constants: current_constants.clone(),
        resume_ip: self.ip,  // Resume at instruction after CaptureCont
        env: self.current_env().clone(),
        operand_stack: self.operand_stack.clone(),
        num_locals: self.current_frame().num_locals,
    };
    self.push(Value::Continuation(Rc::new(cont_data)));
}
```

**Verification**: Test that captures a continuation, does NOT apply it, and
inspects the resulting value.

#### Step 1.4: Implement `ApplyCont` in VM

```rust
Instruction::ApplyCont => {
    let value = self.pop();
    let cont = self.pop();
    
    if let Value::Continuation(cont_data) = cont {
        // Restore execution state
        self.ip = cont_data.resume_ip;
        self.operand_stack = cont_data.operand_stack.clone();
        self.operand_stack.push(value);  // Resume value on stack
        self.restore_env(&cont_data.env);
        // Continue execution (don't return from instruction handler)
    } else {
        return Err("ApplyCont requires a continuation".into());
    }
}
```

**Verification**: Test that captures continuation, stores it, then applies it
with a value. The value should appear where the continuation was captured.

#### Step 1.5: Implement `YieldCont` in VM

```rust
Instruction::YieldCont => {
    let value = self.pop();
    
    // Capture continuation for resumption
    let cont_data = ContinuationData {
        bytecode: current_bytecode.clone(),
        constants: current_constants.clone(),
        resume_ip: self.ip,
        env: self.current_env().clone(),
        operand_stack: self.operand_stack.clone(),
        num_locals: self.current_frame().num_locals,
    };
    
    return Ok(VmResult::Yielded {
        value,
        continuation: Value::Continuation(Rc::new(cont_data)),
    });
}
```

**Note**: This changes `VmResult::Yielded` to carry a `Value::Continuation`
instead of the old `SavedContext`.

**Verification**: Coroutine yields, continuation is stored, resume works.

#### Step 1.6: Update `VmResult` and `Coroutine`

Change `VmResult::Yielded`:

```rust
pub enum VmResult {
    Done(Value),
    Yielded {
        value: Value,
        continuation: Value,  // Always Value::Continuation
    },
}
```

Change `Coroutine`:

```rust
pub struct Coroutine {
    pub closure: Rc<Closure>,
    pub state: CoroutineState,
    pub saved_continuation: Option<Value>,  // Value::Continuation
    pub yielded_value: Option<Value>,
    // Remove: saved_context, saved_env
}
```

**Verification**: All existing coroutine tests pass with new representation.

---

### Phase 2: LIR Continuation Support

**Goal**: Teach the lowerer to emit continuation-aware code.

#### Step 2.1: Add continuation instructions to LIR

In `lir/types.rs`:

```rust
pub enum LirInstr {
    // ... existing ...
    
    /// Capture current continuation into a register
    CaptureCont { dst: Reg },
    
    /// Apply continuation (transfers control, doesn't return)
    ApplyCont { cont: Reg, value: Reg },
}

pub enum Terminator {
    // ... existing ...
    
    /// Yield control with a value (for coroutines)
    Yield { value: Reg },
}
```

**Note**: `Yield` as a terminator, not an instruction. Control leaves the
basic block.

**Verification**: LIR printer shows new constructs.

#### Step 2.2: Lower `HirKind::Yield` to LIR

In `lir/lower.rs`:

```rust
fn lower_yield(&mut self, value_hir: &Hir) -> Result<Reg, String> {
    let value_reg = self.lower_expr(value_hir)?;
    
    // End current block with yield terminator
    self.terminate_block(Terminator::Yield { value: value_reg });
    
    // Create continuation block (where execution resumes)
    let resume_block = self.create_block();
    self.switch_to_block(resume_block);
    
    // Resume value will be on operand stack; load into register
    let resume_reg = self.fresh_reg();
    self.emit(LirInstr::LoadResumeValue { dst: resume_reg });
    
    Ok(resume_reg)
}
```

**Note**: `LoadResumeValue` is a pseudo-instruction that becomes a stack pop
in bytecode. The resume value is pushed by `ApplyCont`.

**Verification**: Simple `(yield 42)` lowers to LIR with Yield terminator.

#### Step 2.3: Emit bytecode for Yield terminator

In `lir/emit.rs`:

```rust
Terminator::Yield { value } => {
    // Ensure value is on operand stack
    self.ensure_on_stack(value);
    // Emit yield instruction
    self.emit_byte(Instruction::YieldCont as u8);
    
    // The next block (resume point) follows immediately
    // When ApplyCont jumps here, the resume value is on stack
}
```

**Verification**: Bytecode disassembly shows `YieldCont` at yield points.

#### Step 2.4: Handle yield in control flow

Ensure yields inside `if`, `while`, `for`, etc. work correctly:

```rust
// Example: (if cond (yield 1) 2)
// Must generate:
//   eval cond
//   branch_false else_block
// then_block:
//   push 1
//   YieldCont        ; control leaves here
// resume_block:      ; control resumes here
//   pop resume_val   ; (or just leave it, as the if result)
//   jump merge
// else_block:
//   push 2
//   jump merge
// merge:
//   ...
```

The key insight: after `YieldCont`, execution resumes at the next instruction.
The lowerer must ensure the resume point is the right place in the control
flow graph.

**Verification**: Tests for yield in if-branches, loop bodies, and function
calls.

---

### Phase 3: Delete CPS Interpreter

**Goal**: Remove the tree-walking CPS interpreter entirely.

#### Step 3.1: Update `coroutine-resume` to use bytecode only

In `primitives/coroutines.rs`:

```rust
pub fn prim_coroutine_resume(args: &[Value], vm: &mut VM) -> LResult<Value> {
    // ...
    match &borrowed.state {
        CoroutineState::Created => {
            // Always use bytecode path
            borrowed.state = CoroutineState::Running;
            let result = vm.execute_coroutine(&borrowed.closure)?;
            // Handle VmResult::Done or VmResult::Yielded
        }
        CoroutineState::Suspended => {
            let continuation = borrowed.saved_continuation.clone()
                .ok_or("Suspended coroutine has no continuation")?;
            let result = vm.apply_continuation(continuation, resume_value)?;
            // Handle result
        }
        // ...
    }
}
```

**Verification**: Coroutine tests pass without CPS interpreter.

#### Step 3.2: Remove `source_ast` from `Closure`

```rust
pub struct Closure {
    pub bytecode: Rc<Vec<u8>>,
    pub arity: Arity,
    pub env: Rc<Vec<Value>>,
    pub num_locals: usize,
    pub num_captures: usize,
    pub constants: Rc<Vec<Value>>,
    pub effect: Effect,
    pub cell_params_mask: u64,
    // REMOVE: source_ast: Option<LambdaAst>,
}
```

The AST was only needed for CPS transformation at runtime. With yields
compiled to bytecode, it's no longer necessary.

**Verification**: Compile succeeds; all tests pass.

#### Step 3.3: Delete CPS modules

Remove entirely:

- `compiler/cps/cps_expr.rs`
- `compiler/cps/transform.rs`
- `compiler/cps/interpreter.rs`
- `compiler/cps/trampoline.rs`
- `compiler/cps/action.rs`
- `compiler/cps/jit.rs`
- `compiler/cps/jit_action.rs`
- `compiler/cps/mixed_calls.rs`
- `compiler/cps/arena.rs`
- `compiler/cps/cont_pool.rs`

Keep (they become simpler or move):

- `compiler/cps/continuation.rs` → Delete; `ContinuationData` is in `value.rs`
- `compiler/cps/primitives.rs` → Merge into `primitives/coroutines.rs`

**Verification**: Build succeeds; `cargo test` passes; no dead code warnings.

#### Step 3.4: Simplify coroutine state

```rust
pub enum CoroutineState {
    Created,
    Running,
    Suspended,
    Done,
    Error(String),
}

pub struct Coroutine {
    pub closure: Rc<Closure>,
    pub state: CoroutineState,
    pub continuation: Option<Value>,  // Value::Continuation when suspended
    pub last_value: Option<Value>,    // Last yielded or returned value
}
```

No more `saved_context`, `saved_env`, `saved_continuation` (old Rc<Continuation>).

**Verification**: All coroutine operations work; state transitions are correct.

---

### Phase 4: JIT Support for Yields

**Goal**: Enable Cranelift to compile yielding functions.

#### Step 4.1: Detect yieldable functions in JIT coordinator

In `compiler/jit_coordinator.rs`:

```rust
pub fn should_jit_compile(closure: &Closure) -> bool {
    // Previously: closure.effect.is_pure()
    // Now: all closures can be JIT compiled
    closure.call_count > JIT_THRESHOLD
}
```

The effect system still exists for optimization hints, but doesn't gate JIT.

**Verification**: JIT coordinator considers yielding functions.

#### Step 4.2: Compile Yield terminator in Cranelift

In `compiler/cranelift/codegen.rs`:

```rust
fn compile_terminator(&mut self, term: &Terminator) -> Result<(), String> {
    match term {
        Terminator::Yield { value } => {
            // 1. Build ContinuationData struct in memory
            let cont_ptr = self.alloc_continuation();
            self.store_resume_ip(cont_ptr);
            self.store_env(cont_ptr);
            self.store_operand_stack(cont_ptr);
            
            // 2. Return JitResult::Yielded { value, continuation }
            let value_ir = self.load_reg(*value);
            self.build_yield_return(value_ir, cont_ptr);
        }
        // ... other terminators ...
    }
}
```

**Note**: JIT-compiled functions return `JitResult` (like `VmResult`), not
raw values. The trampoline handles the result.

**Verification**: Simple yielding function JIT-compiles; execution works.

#### Step 4.3: Handle continuation resume in JIT

When resuming a continuation that points into JIT-compiled code:

```rust
pub fn apply_jit_continuation(cont: &ContinuationData, value: Value) -> JitResult {
    // The continuation's resume_ip is an offset into native code
    // We need to call back into the JIT-compiled function at that offset
    
    // This requires the JIT to generate multiple entry points:
    // - Main entry (ip=0)
    // - Resume entry per yield point
    
    let entry_fn = get_resume_entry(cont.bytecode_id, cont.resume_ip);
    entry_fn(cont.env, value)
}
```

**Implementation detail**: Each yield point in a JIT-compiled function
becomes a separate entry point. The `resume_ip` indexes into a table of
entry points.

**Verification**: JIT-compiled coroutine can yield and resume.

#### Step 4.4: Optimize pure sections

If effect analysis shows a region is pure (no yields), the JIT can:

- Avoid continuation capture overhead
- Use native call stack for internal calls
- Inline aggressively

```rust
fn compile_block(&mut self, block: &BasicBlock) -> Result<(), String> {
    if block.effect.is_pure() {
        // Fast path: no continuation machinery
        self.compile_pure_block(block)
    } else {
        // Slow path: continuation-aware compilation
        self.compile_yielding_block(block)
    }
}
```

**Verification**: Benchmark shows pure code is not penalized by continuation
support.

---

### Phase 5: Cleanup and Optimization

**Goal**: Polish the implementation and optimize common cases.

#### Step 5.1: Stack frame reuse for non-yielding calls

If a call is to a function that cannot yield (effect is `Pure`), use the
native call stack:

```rust
Instruction::Call { arity } if target_is_pure => {
    // Direct call, no continuation capture
    let result = self.direct_call(func, args)?;
    self.push(result);
}

Instruction::Call { arity } => {
    // Must capture continuation for potential yield
    // ... existing implementation ...
}
```

**Verification**: Benchmark shows pure function calls are not slower.

#### Step 5.2: Continuation pooling

Reuse `ContinuationData` allocations:

```rust
thread_local! {
    static CONT_POOL: RefCell<Vec<Box<ContinuationData>>> = RefCell::new(Vec::new());
}

fn alloc_continuation() -> Rc<ContinuationData> {
    CONT_POOL.with(|pool| {
        if let Some(mut cont) = pool.borrow_mut().pop() {
            // Reuse existing allocation
            cont.clear();
            Rc::new(*cont)
        } else {
            Rc::new(ContinuationData::new())
        }
    })
}
```

**Verification**: Benchmark shows reduced allocation overhead in tight
yield loops.

#### Step 5.3: Remove dead code and unused fields

Audit all modules for:

- Unused imports
- Dead enum variants
- Obsolete comments referencing old CPS interpreter
- Test helpers that test removed functionality

Run `cargo clippy --workspace -- -D warnings` and fix all issues.

**Verification**: Clean clippy; no warnings.

#### Step 5.4: Update documentation

- Update `AGENTS.md` to reflect single execution path
- Update `docs/CPS_DESIGN.md` or replace with this document
- Add architecture diagram showing new flow
- Document `Value::Continuation` in language guide

**Verification**: Documentation accurately describes implementation.

---

## Migration Checklist

Track progress by checking off completed steps:

### Phase 0: Prerequisites
- [x] 0.1: NaN-boxing Value merged
- [x] 0.2: Continuation usage audited
- [x] 0.3: Comprehensive coroutine tests added

### Phase 1: First-class Continuations
- [x] 1.1: `Value::Continuation` defined (`ContinuationData`, `ContinuationFrame`)
- [x] 1.2: Frame chain mechanism in VM (Yield captures frame, Call appends caller frame)
- [x] 1.3: `resume_continuation` replays frame chain
- [x] 1.4: `VmResult::Yielded` carries continuation value

Note: The original plan (1.2-1.5) was superseded by the frame-chain approach.
Instead of explicit `CaptureCont`/`ApplyCont` instructions, continuations are
built incrementally as yields propagate through call boundaries.

### Phase 2: Delete CPS Interpreter
- [x] 2.1: Removed `compiler/cps/` (~4,400 lines)
- [x] 2.2: Simplified `Coroutine` struct (7 fields → 4)
- [x] 2.3: Single execution path (bytecode only)
- [x] 2.4: Migrated `yielded_value` to new Value type

### Phase 3: Harden Continuations
- [x] 3.1: Exception handler state saved in continuation frames
- [x] 3.2: `ContinuationData` frame ordering optimized (O(1) append)
- [x] 3.3: Edge case tests (handler-case+yield, deep call chains, tail calls)
- [x] 3.4: Documentation updated
- [x] 3.5: Exception check at start of instruction loop (for cross-frame propagation)
- [x] 3.6: Tail call handling in `execute_bytecode_from_ip_with_state`

### Phase 4: LIR Continuation Instructions (future)
- [ ] Yield as LIR terminator
- [ ] Explicit continuation capture/apply in LIR
- [ ] Prerequisite: JIT consumes LIR (currently uses old Expr AST)

### Phase 5: JIT Support for Yields (future)
- [ ] Rewrite JIT to consume LIR
- [ ] Compile Yield terminator in Cranelift
- [ ] Continuation resume in JIT

---

## Risks and Mitigations

### Risk: Performance regression for non-yielding code

**Mitigation**: Phase 5.1 ensures pure calls use native stack. Effect system
guides optimization.

### Risk: Complex control flow with yields

**Mitigation**: Phase 2.4 explicitly tests all control flow combinations.
The LIR basic block structure naturally handles this.

### Risk: Breaking existing coroutine behavior

**Mitigation**: Phase 0.3 adds comprehensive tests before any changes. Each
phase ends with "all tests pass" verification.

### Risk: JIT complexity explosion

**Mitigation**: Phase 4 is independent. We can ship Phases 1-3 without JIT
yield support. Bytecode is always the fallback.

---

## Success Criteria

The rework is complete when:

1. All coroutine tests pass
2. No CPS interpreter code remains
3. `Closure` has no `source_ast` field
4. JIT can compile yielding functions (Phase 4)
5. Benchmark shows no regression for non-yielding code
6. Documentation is updated

---

## Appendix: Bytecode Comparison

### Before (current)

Yielding function uses CPS interpreter:
```
; Bytecode never executed for yielding closures
; Instead: Expr → CpsExpr → tree-walk
```

### After (target)

Yielding function compiles to bytecode:
```
; (define (gen) (yield 1) (yield 2) 3)
entry:
    LoadConst 1       ; push 1
    YieldCont         ; yield, capture continuation
resume_1:             ; execution resumes here
    Pop               ; discard resume value
    LoadConst 2       ; push 2
    YieldCont         ; yield, capture continuation  
resume_2:             ; execution resumes here
    Pop               ; discard resume value
    LoadConst 3       ; push 3
    Return
```

The key difference: yields are bytecode instructions, not interpreter mode
switches. The VM handles `YieldCont` by capturing state and returning to the
caller. Resume happens by `ApplyCont` which restores state and continues.
