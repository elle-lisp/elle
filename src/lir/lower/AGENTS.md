# lir/lower

HIR to LIR lowering: explicit control flow, binding slot allocation, lbox operations, and escape analysis.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `Binding` references to concrete slot indices
- Emit lbox operations for mutable captures
- Perform escape analysis for scope allocation
- Compute compile-time scope allocation statistics

Does NOT:
- Resolve bindings (that's HIR)
- Execute code (that's VM)
- Perform optimization (future work)

## Key types

| Type | Purpose |
|------|---------|
| `Lowerer` | Main struct that transforms HIR → LIR |
| `LirFunction` | Compilation unit: blocks, constants, metadata, docstring |
| `BasicBlock` | Instructions + terminator |
| `LirInstr` | Individual operation |
| `SpannedInstr` | `LirInstr` + `Span` for source tracking |
| `Terminator` | How block exits: `Return`, `Jump`, `Branch`, `Yield` |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `BlockLowerContext` | Active block for `break` lowering (block_id, result_reg, exit_label, region_depth_at_entry) |
| `ScopeStats` | Compile-time scope allocation statistics |

## Data flow

```
HIR + spans
    │
    ▼
Lowerer
    ├─► allocate slots for bindings (HashMap<Binding, u16>)
     ├─► emit MakeLBox for captured locals (binding.needs_lbox())
    ├─► lower control flow to jumps
    ├─► emit LoadCapture/StoreCapture for upvalues
    ├─► perform escape analysis for scope allocation
    └─► propagate HIR spans to SpannedInstr
    │
    ▼
LirFunction (basic blocks with SpannedInstr)
```

The lowerer reads binding metadata directly from `Binding` objects (via `binding.needs_lbox()`, `binding.is_global()`, `binding.name()`, etc.) rather than looking up a separate bindings HashMap.

## Source location tracking

`SpannedInstr` wraps `LirInstr` with a `Span` for source location tracking:

```rust
pub struct SpannedInstr {
    pub instr: LirInstr,
    pub span: Span,
}
```

The lowerer propagates HIR spans to LIR instructions. The emitter builds a `LocationMap` that maps bytecode offsets to source locations. This map is stored in `Closure.location_map` and used by the VM for error reporting.

## Escape analysis for scope allocation

Scope allocation uses `RegionEnter` and `RegionExit` instructions to mark allocation regions. The lowerer performs escape analysis to determine when a scope's allocations are safe to release at scope exit.

**Escape analysis conditions (all must hold for `let`/`letrec`):**
1. No binding is captured by a nested lambda
2. Body cannot suspend (`may_suspend()`)
3. Body result is provably a NaN-boxed immediate (`result_is_safe`)
4. Body contains no dangerous `set` to bindings outside the scope (`body_contains_dangerous_outward_set`) — an outward set is dangerous only if the assigned value is not provably immediate
5. Body contains no escaping `break` (`body_contains_escaping_break`) — breaks targeting blocks inside the scope are safe; only breaks targeting outer blocks are dangerous

For `block`: conditions 1-4 plus all break values targeting this block are safe immediates and no escaping breaks.

**`result_is_safe` tiers:**
- **Tier 1**: Whitelisted immediate-returning primitives (`length`, `empty?`, `abs`, `floor`, `ceil`, `round`, `type`, `type-of`, type predicates)
- **Tier 2**: Intrinsic operations (`BinOp`, `CmpOp`, `UnaryOp`) with correct arity
- **Tier 3**: `Var` referencing an outer binding or a scope binding whose init is provably immediate
- **Tier 4**: Nested `Let`/`Letrec`/`Block` where the inner result is recursively safe
- **Tier 5**: `Match` where all arm bodies are recursively safe
- **Tier 6**: `While` (always returns nil)
- **Tier 7**: Breaks targeting blocks inside the scope (safe — don't exit the region)
- **Tier 8**: Outward sets with provably immediate values (safe — don't escape heap pointers)

**Compile-time scope stats** (`ScopeStats`): The lowerer counts how many scopes were analyzed, how many qualified for scope allocation, and the first-failing condition for each rejected scope (captured, suspends, unsafe-result, outward-set, break). Access via `lowerer.scope_stats()` after `lower()` completes. Set `ELLE_SCOPE_STATS=1` to print stats to stderr during compilation.

**Known limitations and why they exist:**

- **`suspends` (condition 2)**: Any let body that calls a `Polymorphic`-signal
  function (e.g., `map`, `filter`, `fold` with a callback) fails this condition.
  Fixing this requires knowing the concrete signal of the callback at the call
  site — i.e., monomorphization or signal polymorphism tracking. Not feasible
  without interprocedural analysis.

- **`unsafe-result` (condition 3)**: Calls to user-defined functions fail
  `result_is_safe` because we don't know their return type at the call site.
  Fixing this requires return-type inference or a return-type annotation system.
  The whitelist in `IMMEDIATE_PRIMITIVES` covers built-in primitives only.

These are accepted limitations. The analysis is maximally conservative to
avoid use-after-free. False negatives (missed optimizations) are preferable
to false positives (use-after-free bugs).

## Yield as terminator

`Terminator::Yield { value, resume_label }` correctly models that yield suspends execution and resumes in a new block. The lowerer:

1. Emits `Terminator::Yield` to end the current block
2. Creates a new block at `resume_label`
3. Emits `LoadResumeValue` as the first instruction of the resume block

The emitter preserves stack state across the yield boundary via `yield_stack_state`. This ensures intermediate values computed before yield (e.g., the `1` in `(+ 1 (yield 2) 3)`) survive into the resume block.

## Block/Break lowering

`HirKind::Block` lowers to a result register + exit label pattern:
1. Allocate `result_reg` and `exit_label`
2. Push `BlockLowerContext { block_id, result_reg, exit_label, region_depth_at_entry }`
3. Lower body, move result to `result_reg`
4. Pop context, jump to `exit_label`, start new block at `exit_label`

`HirKind::Break` lowers to Move + Jump:
1. Find target block's `result_reg` and `exit_label` via `block_lower_contexts`
2. Lower value, move to `result_reg`, jump to `exit_label`
3. Emit compensating `RegionExit` instructions for each region entered between break site and target block
4. Start unreachable dead-code block

No new bytecode instructions — break compiles to existing Move + Jump + RegionExit.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~280 | `Lowerer` struct, context, entry point, `can_scope_allocate_*` analysis |
| `expr.rs` | ~457 | Expression lowering: literals, operators, calls |
| `binding.rs` | ~280 | Binding forms: `let`, `def`, `var`, `fn` |
| `lambda.rs` | ~250 | fn lowering, closure capture, lbox wrapping |
| `control.rs` | ~200 | Control flow: `if`, `begin`, `match` |
| `pattern.rs` | ~200 | Pattern matching lowering |
| `escape.rs` | ~693 | Escape analysis helpers: `result_is_safe`, `body_contains_dangerous_outward_set`, `body_contains_escaping_break`, `all_break_values_safe` |
| `decision.rs` | ~100 | Decision tree compilation for pattern matching |

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
| `CarDestructure` | value → car | Car of cons, signals error if not a cons |
| `CdrDestructure` | value → cdr | Cdr of cons, signals error if not a cons |
| `ArrayMutRefDestructure` | array → elem | Array element by immediate u16 index, signals error if wrong type or out of bounds |
| `IsArray` | value → bool | Type check: is value an array? (for pattern matching) |
| `IsStruct` | value → bool | Type check: is value a struct or @struct? (for pattern matching) |
| `ArrayLen` | array → int | Get array length (for pattern matching) |
| `TableGetOrNil` | struct → value | Get key from struct/@struct, nil if missing/wrong type — used by match (u16 const_idx operand) |
| `TableGetDestructure` | struct → value | Get key from struct/@struct, signals error if missing/wrong type — used by binding forms (u16 const_idx operand) |
| `PushParamFrame` | (none) | Push a new parameter binding frame (operand: count u8) |
| `PopParamFrame` | (none) | Pop the current parameter binding frame |
| `RegionEnter` | (none) | Push scope mark on FiberHeap (no-op for root fiber) |
| `RegionExit` | (none) | Pop scope mark and release scoped objects (no-op for root fiber) |

## Invariants

1. **Each register assigned exactly once.** SSA form. If you see a register used before definition, lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Yield`, or `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with "unknown binding," the HIR→LIR mapping is incomplete. The key is `Binding` (hashed by `Value::to_bits()`), the value is `u16` slot index.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside fn bodies, captures and parameters are upvalues; they use LoadCapture, not LoadLocal.

5. **`lbox_params_mask` is set for mutable parameters.** Bit i set means parameter i needs lbox wrapping at call time.

6. **`lbox_locals_mask` is set for locals that need lboxes.** Bit i set means locally-defined variable i (0-indexed from the first local after params) needs lbox wrapping because it's captured by a nested closure or mutated via `set!`. The JIT uses this to skip `LocalLBox` heap allocation for non-captured, non-mutated `let` bindings. The VM interpreter does not use this mask (it lbox-wraps all locals unconditionally). Both masks are limited to 64 entries (`u64`).

7. **Docstring is threaded from HIR.** `LirFunction.doc` is copied from `HirKind::Lambda.doc` during lowering. The emitter preserves it into `Closure.doc` without encoding it in bytecode.

## When to modify

- **Adding a new special form**: Add a case in `expr.rs::lower_expr`, implement `lower_your_form` method
- **Changing binding lowering**: Update `binding.rs`
- **Changing control flow**: Update `control.rs`
- **Changing pattern matching**: Update `pattern.rs` and `decision.rs`
- **Changing escape analysis**: Update `escape.rs` and `mod.rs::can_scope_allocate_*`
- **Adding new bytecode instructions**: Update `expr.rs`, `control.rs`, `binding.rs`, or `lambda.rs` to emit them

## Common pitfalls

- **Forgetting to allocate slots**: Every binding used in the function must have a slot allocated via `allocate_slot()`
- **Mixing LoadLocal and LoadCapture**: Inside lambdas, upvalues use LoadCapture; locals use LoadLocal
- **Not emitting lbox operations**: If a binding needs an lbox, emit `MakeLBox` before storing
- **Not propagating spans**: Every emitted instruction should carry the source span from the HIR node
- **Forgetting region cleanup**: If `RegionEnter` is emitted, ensure `RegionExit` is emitted at scope exit
- **Not handling break compensation**: When emitting `break`, emit compensating `RegionExit` instructions for each region entered between break site and target
