# lir

Low-level Intermediate Representation. SSA form with virtual registers
and basic blocks. Architecture-independent but close to target.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `Binding` references to concrete slot indices
- Emit lbox operations for mutable captures
- Produce bytecode via `Emitter`

Does NOT:
- Resolve bindings (that's HIR)
- Execute code (that's VM)
- Perform optimization (future work)

## Interface

| Type | Purpose |
|------|---------|
| `LirFunction` | Compilation unit: blocks, constants, metadata, docstring, syntax, yield/call-site info |
| `BasicBlock` | Instructions + terminator |
| `LirInstr` | Individual operation |
| `SpannedInstr` | `LirInstr` + `Span` for source tracking |
| `SpannedTerminator` | `Terminator` + `Span` for source tracking |
| `Terminator` | How block exits: `Return`, `Jump`, `Branch`, `Yield` |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `YieldPointInfo` | Metadata for a yield point: resume IP and live registers |
| `CallSiteInfo` | Metadata for a call site: resume IP and live registers (for yield-through-call) |
| `Lowerer` | HIR → LIR |
| `ScopeStats` | Compile-time scope allocation statistics |
| `Emitter` | LIR → (Bytecode, yield_points, call_sites) |

## Data flow

```
HIR + spans
    │
    ▼
Lowerer (&BindingArena)
    ├─► allocate slots for bindings (HashMap<Binding, u16>)
    ├─► emit MakeLBox for captured locals (arena.get(b).needs_lbox())
    ├─► lower control flow to jumps
    ├─► emit LoadCapture/StoreCapture for upvalues
    └─► propagate HIR spans to SpannedInstr
    │
    ▼
LirFunction (basic blocks with SpannedInstr)
    │
    ▼
Emitter
    ├─► simulate stack for register→stack translation
    ├─► patch jump offsets
    ├─► emit Instruction bytes
    ├─► build LocationMap from SpannedInstr spans
    ├─► collect YieldPointInfo (resume IP + live registers at each yield)
    └─► collect CallSiteInfo (resume IP + live registers at each call in may_suspend functions)
    │
    ▼
(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)
    │
    ├─► Bytecode + LocationMap → VM execution
    │
    └─► YieldPointInfo + CallSiteInfo → LirFunction.yield_points/call_sites
        → JIT compilation (for side-exit code generation)
```

The lowerer reads binding metadata via `&BindingArena` (passed to `Lowerer::new`):
`arena.get(b).needs_lbox()`, `arena.get(b).name`, etc.

## Source location tracking

`SpannedInstr` wraps `LirInstr` with a `Span` for source location tracking:

```rust
pub struct SpannedInstr {
    pub instr: LirInstr,
    pub span: Span,
}
```

The lowerer propagates HIR spans to LIR instructions. The emitter builds a
`LocationMap` that maps bytecode offsets to source locations. This map is
stored in `Closure.location_map` and used by the VM for error reporting.

## Dependents

- `pipeline.rs` - uses `Lowerer` and `Emitter`
- `vm/` - executes the emitted bytecode

## Invariants

1. **Each register assigned exactly once.** SSA form. If you see a register
    used before definition, lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Yield`,
    or `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with
    "unknown binding," the HIR→LIR mapping is incomplete. The key is `Binding`
    (hashed by `Value::to_bits()`), the value is `u16` slot index.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside fn bodies,
    captures and parameters are upvalues; they use LoadCapture, not LoadLocal.

5. **`lbox_params_mask` is set for mutable parameters.** Bit i set means
     parameter i needs lbox wrapping at call time.

6. **`lbox_locals_mask` is set for locals that need lboxes.** Bit i set means
     locally-defined variable i (0-indexed from the first local after params)
     needs lbox wrapping because it's captured by a nested closure or mutated
     via `set!`. The JIT uses this to skip `LocalLBox` heap allocation for
     non-captured, non-mutated `let` bindings. The VM interpreter does not
     use this mask (it lbox-wraps all locals unconditionally). Both masks
     are limited to 64 entries (`u64`).

7. **Yield is a block terminator, not an instruction.** `Terminator::Yield`
    splits the block: the current block ends with yield, and a new resume block
    begins. The resume block starts with `LoadResumeValue` to capture the value
    passed to `coro/resume`.

8. **Docstring and syntax are threaded from HIR.** `LirFunction.doc` and
      `LirFunction.syntax` are copied from `HirKind::Lambda.doc` and
      `HirKind::Lambda.syntax` during lowering. The emitter preserves both
      into `Closure.doc` and `Closure.syntax` without encoding them in bytecode.

9. **Yield point metadata is collected during emission.** `Emitter::emit()`
     returns `(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)`. The caller
     must attach these to `LirFunction.yield_points` and `LirFunction.call_sites`
     before storing the function on a `Closure`. The JIT reads this metadata
     to generate side-exit code.

10. **Call site metadata is only populated for may_suspend functions.**
     `Emitter.current_func_may_suspend` gates call site recording. For
     non-suspending functions, `call_sites` is empty. This avoids overhead
     for silent functions that can never yield.

9. **Yield point metadata is collected during emission.** `Emitter::emit()`
    returns `(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)`. The caller
    must attach these to `LirFunction.yield_points` and `LirFunction.call_sites`
    before storing the function on a `Closure`. The JIT reads this metadata
    to generate side-exit code.

10. **Call site metadata is only populated for may_suspend functions.**
     `Emitter.current_func_may_suspend` gates call site recording. For
     non-suspending functions, `call_sites` is empty. This avoids overhead
     for silent functions that can never yield.

## Key instructions

| Instruction | Stack effect | Notes |
|-------------|--------------|-------|
| `LoadLocal` | → value | Load from stack slot |
| `StoreLocal` | value → value | Store to slot, keep on stack |
| `LoadCapture` | → value | From closure env, auto-unwraps LocalLBox |
| `LoadCaptureRaw` | → lbox | From closure env, preserves lbox (for forwarding) |
| `StoreCapture` | value → | Into closure env, handles lboxes |
| `MakeLBox` | value → lbox | Wrap in LocalLBox |
| `MakeClosure` | caps... → closure | Pops N captures, creates closure |
| `EmptyList` | → empty_list | Push Value::EMPTY_LIST (truthy, unlike Nil) |
| `LoadResumeValue` | → value | First instruction in yield resume block |
| `CarOrNil` | value → car | Car of cons, or nil if not a cons |
| `CdrOrNil` | value → cdr | Cdr of cons, or EMPTY_LIST if not a cons |
| `ArrayRefOrNil` | array → elem | Array element by immediate u16 index, or nil if out of bounds |
| `IsArray` | value → bool | Type check: is value an array (immutable)? (for pattern matching) |
| `IsArrayMut` | value → bool | Type check: is value an @array (mutable)? (for pattern matching) |
| `IsStruct` | value → bool | Type check: is value a struct (immutable)? (for pattern matching) |
| `IsStructMut` | value → bool | Type check: is value an @struct (mutable)? (for pattern matching) |
| `ArrayLen` | array → int | Get array length (for pattern matching) |
| `TableGetOrNil` | table → value | Get key from table/struct, or nil if missing/wrong type (u16 const_idx operand) |
| `PushParamFrame` | (none) | Push a new parameter binding frame (operand: count u8) |
| `PopParamFrame` | (none) | Pop the current parameter binding frame |
| `RegionEnter` | (none) | Push scope mark on FiberHeap (effective for all fibers including root) |
| `RegionExit` | (none) | Pop scope mark and release scoped objects (effective for all fibers including root) |

## Yield and Call Site Metadata

The emitter collects two types of metadata during bytecode emission:

### YieldPointInfo

Recorded when a `Terminator::Yield` is emitted:
- `resume_ip: usize` — Bytecode offset to resume at (the instruction after the Yield opcode)
- `stack_regs: Vec<Reg>` — Virtual registers on the operand stack at yield time, bottom-to-top

The JIT uses this to spill live registers to a stack slot and call the yield runtime helper.

### CallSiteInfo

Recorded when a `LirInstr::Call` is emitted in a function where `signal.may_suspend()`:
- `resume_ip: usize` — Bytecode offset after the Call instruction (where the interpreter resumes if the callee yields)
- `stack_regs: Vec<Reg>` — Virtual registers on the operand stack after popping func/args but before pushing the result

This matches the interpreter's stack state when yield propagates through a call. The JIT uses this to build the caller's `SuspendedFrame` when a callee yields (yield-through-call).

## Allocation regions

`RegionEnter` and `RegionExit` are no-register, no-stack-effect instructions
that push/pop scope marks on the current FiberHeap. In the VM, they call
`region_enter()`/`region_exit()`, which are effective for all fibers including
root (after issue-525, the root fiber always has a FiberHeap installed).

The lowerer emits these instructions when escape analysis (in `lower/escape.rs`)
determines the scope's allocations are safe to release at scope exit.
Function bodies never get region instructions.

**Escape analysis conditions (all must hold):**
1. No binding is captured by a nested lambda
2. Body cannot suspend (`may_suspend()`)
3. Body result is provably a immediate (`result_is_safe`)
4. Body contains no dangerous `set` to bindings outside the scope
   (`body_contains_dangerous_outward_set`) — Tier 8: an outward set is
   dangerous only if the assigned value is not provably immediate
5. Body contains no escaping `break` (`body_contains_escaping_break`) —
   Tier 7: breaks targeting blocks inside the scope are safe (they don't
   exit the scope's region); only breaks targeting outer blocks are dangerous

For `let`/`letrec`: all six conditions. `letrec` delegates to `let`.
For `block`: conditions 1-4 plus all break values targeting this block are
safe immediates (Tier 6) and no escaping breaks (Tier 7).

`result_is_safe` takes `scope_bindings: &[(Binding, &Hir)]` — the
bindings introduced by the let/letrec being analyzed. It returns
`true` for: literals, `Var` referencing an outer binding (not in
scope set), `Var` referencing a scope binding whose init is provably
immediate (Tier 3), `if`/`begin`/`cond`/`and`/`or` where all result
positions are recursively safe, calls to intrinsics (`BinOp`, `CmpOp`,
`UnaryOp`) with correct arity (including unary `-` as `Neg`, Tier 2),
calls to whitelisted immediate-returning primitives (Tier 1),
nested `Let`/`Letrec`/`Block` where the inner result is recursively
safe (Tier 4), `Match` where all arm bodies are recursively safe
(Tier 5), and `While` which always returns nil (Tier 6). For nested
let/letrec, scope_bindings is extended with the inner let's bindings
before recursing (inner bindings are allocated within the outer
scope's region). For blocks, `scope_bindings` is unchanged (blocks
introduce no bindings).

**Tier 1 primitive whitelist** (in `intrinsics.rs`): `length`, `empty?`,
`abs`, `floor`, `ceil`, `round`, `type`, `type-of`, and all type
predicates (`nil?`, `pair?`, `string?`, `number?`, `array?`, etc.).
These are primitives that always return int, float, bool, or keyword
on success. Full list in `IMMEDIATE_PRIMITIVES` const.

**Known limitation (E5/E6):** If the body passes a scope-allocated
value to a function that stores it externally, the analysis cannot
detect this. Requires interprocedural analysis. Accepted for Tier 0.

`break` emits compensating `RegionExit` instructions for each region entered
between the break site and the target block. The lowerer tracks `region_depth`
and each `BlockLowerContext` records `region_depth_at_entry`.

**Compile-time scope stats** (`ScopeStats`): The lowerer counts how many
scopes were analyzed, how many qualified for scope allocation, and the
first-failing condition for each rejected scope (captured, suspends,
unsafe-result, outward-set, break). Access via `lowerer.scope_stats()`
after `lower()` completes. Set `ELLE_SCOPE_STATS=1` to print stats to
stderr during compilation.

## Yield as terminator

`Terminator::Yield { value, resume_label }` correctly models that yield
suspends execution and resumes in a new block. The lowerer:

1. Emits `Terminator::Yield` to end the current block
2. Creates a new block at `resume_label`
3. Emits `LoadResumeValue` as the first instruction of the resume block

The emitter preserves stack state across the yield boundary via
`yield_stack_state`. This ensures intermediate values computed before yield
(e.g., the `1` in `(+ 1 (yield 2) 3)`) survive into the resume block.

**Yield point metadata:** When the emitter encounters a `Terminator::Yield`,
it records a `YieldPointInfo` containing:
- `resume_ip`: Bytecode offset to resume at (the instruction after Yield)
- `stack_regs`: Virtual registers on the operand stack at yield time

This metadata is collected in `Emitter.yield_points` and returned alongside
the bytecode. The JIT uses this to generate side-exit code that spills live
registers and calls the yield runtime helper.

## Block/Break lowering

`HirKind::Block` lowers to a result register + exit label pattern:
1. Allocate `result_reg` and `exit_label`
2. Push `BlockLowerContext { block_id, result_reg, exit_label }`
3. Lower body, move result to `result_reg`
4. Pop context, jump to `exit_label`, start new block at `exit_label`

`HirKind::Break` lowers to Move + Jump:
1. Find target block's `result_reg` and `exit_label` via `block_lower_contexts`
2. Lower value, move to `result_reg`, jump to `exit_label`
3. Start unreachable dead-code block

No new bytecode instructions — break compiles to existing Move + Jump.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 20 | Re-exports |
| `types.rs` | 270 | `LirFunction`, `LirInstr`, `Reg`, `Label`, etc. |
| `intrinsics.rs` | ~120 | `IntrinsicOp` enum, intrinsics map, `IMMEDIATE_PRIMITIVES` whitelist, `build_immediate_primitives()` |
| `lower/mod.rs` | ~280 | `Lowerer` struct, context, entry point, `can_scope_allocate_*` analysis |
| `lower/escape.rs` | ~693 | Escape analysis helpers: `result_is_safe`, `body_contains_dangerous_outward_set`, `body_contains_escaping_break`, `all_break_values_safe`, `all_breaks_have_safe_values` |
| `lower/expr.rs` | ~457 | Expression lowering: literals, operators, calls |
| `lower/binding.rs` | ~280 | Binding forms: `let`, `def`, `var`, `fn` |
| `lower/lambda.rs` | ~250 | fn lowering, closure capture, lbox wrapping |
| `lower/control.rs` | ~200 | Control flow: `if`, `begin`, `match` |
| `lower/pattern.rs` | ~1135 | Pattern matching lowering: decision tree walking, constructor tests |
| `lower/access.rs` | ~85 | Access path loading: navigate cons/array/struct to extract matched values |
| `emit/mod.rs` | ~820 | `Emitter`, LIR→Bytecode instruction encoding |
| `emit/stack.rs` | ~85 | Stack simulation helpers: `push_reg`, `pop`, `ensure_on_top`, `ensure_binary_on_top` |

## Constants

`LirConst` represents compile-time constants. Note: `LirConst::Nil` and
`LirConst::EmptyList` are distinct. Nil is falsy, EmptyList is truthy. Lists
terminate with EmptyList, not Nil.
