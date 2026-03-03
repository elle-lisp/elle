# lir

Low-level Intermediate Representation. SSA form with virtual registers
and basic blocks. Architecture-independent but close to target.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `Binding` references to concrete slot indices
- Emit cell operations for mutable captures
- Produce bytecode via `Emitter`

Does NOT:
- Resolve bindings (that's HIR)
- Execute code (that's VM)
- Perform optimization (future work)

## Interface

| Type | Purpose |
|------|---------|
| `LirFunction` | Compilation unit: blocks, constants, metadata, docstring |
| `BasicBlock` | Instructions + terminator |
| `LirInstr` | Individual operation |
| `SpannedInstr` | `LirInstr` + `Span` for source tracking |
| `SpannedTerminator` | `Terminator` + `Span` for source tracking |
| `Terminator` | How block exits: `Return`, `Jump`, `Branch`, `Yield` |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `Lowerer` | HIR → LIR |
| `ScopeStats` | Compile-time scope allocation statistics |
| `Emitter` | LIR → Bytecode + LocationMap |

## Data flow

```
HIR + spans
    │
    ▼
Lowerer
    ├─► allocate slots for bindings (HashMap<Binding, u16>)
    ├─► emit MakeCell for captured locals (binding.needs_cell())
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
    └─► build LocationMap from SpannedInstr spans
    │
    ▼
Bytecode + LocationMap
```

The lowerer reads binding metadata directly from `Binding` objects (via
`binding.needs_cell()`, `binding.is_global()`, `binding.name()`, etc.)
rather than looking up a separate bindings HashMap.

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

5. **`cell_params_mask` is set for mutable parameters.** Bit i set means
    parameter i needs cell wrapping at call time.

6. **`cell_locals_mask` is set for locals that need cells.** Bit i set means
    locally-defined variable i (0-indexed from the first local after params)
    needs cell wrapping because it's captured by a nested closure or mutated
    via `set!`. The JIT uses this to skip `LocalCell` heap allocation for
    non-captured, non-mutated `let` bindings. The VM interpreter does not
    use this mask (it cell-wraps all locals unconditionally). Both masks
    are limited to 64 entries (`u64`).

7. **Yield is a block terminator, not an instruction.** `Terminator::Yield`
    splits the block: the current block ends with yield, and a new resume block
    begins. The resume block starts with `LoadResumeValue` to capture the value
    passed to `coro/resume`.

8. **Docstring is threaded from HIR.** `LirFunction.doc` is copied from
    `HirKind::Lambda.doc` during lowering. The emitter preserves it into
    `Closure.doc` without encoding it in bytecode.

## Key instructions

| Instruction | Stack effect | Notes |
|-------------|--------------|-------|
| `LoadLocal` | → value | Load from stack slot |
| `StoreLocal` | value → value | Store to slot, keep on stack |
| `LoadCapture` | → value | From closure env, auto-unwraps LocalCell |
| `LoadCaptureRaw` | → cell | From closure env, preserves cell (for forwarding) |
| `StoreCapture` | value → | Into closure env, handles cells |
| `MakeCell` | value → cell | Wrap in LocalCell |
| `MakeClosure` | caps... → closure | Pops N captures, creates closure |
| `EmptyList` | → empty_list | Push Value::EMPTY_LIST (truthy, unlike Nil) |
| `LoadResumeValue` | → value | First instruction in yield resume block |
| `CarOrNil` | value → car | Car of cons, or nil if not a cons |
| `CdrOrNil` | value → cdr | Cdr of cons, or EMPTY_LIST if not a cons |
| `ArrayRefOrNil` | array → elem | Array element by immediate u16 index, or nil if out of bounds |
| `IsArray` | value → bool | Type check: is value an array? (for pattern matching) |
| `IsTable` | value → bool | Type check: is value a table or struct? (for pattern matching) |
| `ArrayLen` | array → int | Get array length (for pattern matching) |
| `TableGetOrNil` | table → value | Get key from table/struct, or nil if missing/wrong type (u16 const_idx operand) |
| `RegionEnter` | (none) | Push scope mark on FiberHeap (no-op for root fiber) |
| `RegionExit` | (none) | Pop scope mark and release scoped objects (no-op for root fiber) |

## Allocation regions

`RegionEnter` and `RegionExit` are no-register, no-stack-effect instructions
that push/pop scope marks on the current FiberHeap. In the VM, they call
`region_enter()`/`region_exit()` which are no-ops for the root fiber
(no FiberHeap installed).

The lowerer emits these instructions when escape analysis (in `lower/escape.rs`)
determines the scope's allocations are safe to release at scope exit.
Function bodies never get region instructions.

**Escape analysis conditions (all must hold):**
1. No binding is captured by a nested lambda
2. Body cannot suspend (`may_suspend()`)
3. Body result is provably a NaN-boxed immediate (`result_is_safe`)
4. Body contains no dangerous `set` to bindings outside the scope
   (`body_contains_dangerous_outward_set`) — Tier 8: an outward set is
   dangerous only if the assigned value is not provably immediate
5. Body contains no `break` (break carries a value past RegionExit)

For `let`/`letrec`: all five conditions. `letrec` delegates to `let`.
For `block`: conditions 1-4 plus no `break` nodes in the body.

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
| `lower/escape.rs` | ~434 | Escape analysis helpers: `result_is_safe`, `body_contains_dangerous_outward_set`, `body_contains_break` |
| `lower/expr.rs` | ~457 | Expression lowering: literals, operators, calls |
| `lower/binding.rs` | ~280 | Binding forms: `let`, `def`, `var`, `fn` |
| `lower/lambda.rs` | ~250 | fn lowering, closure capture, cell wrapping |
| `lower/control.rs` | ~200 | Control flow: `if`, `begin`, `match` |
| `lower/pattern.rs` | ~200 | Pattern matching lowering |
| `emit.rs` | 902 | `Emitter`, LIR→Bytecode with stack simulation |

## Constants

`LirConst` represents compile-time constants. Note: `LirConst::Nil` and
`LirConst::EmptyList` are distinct. Nil is falsy, EmptyList is truthy. Lists
terminate with EmptyList, not Nil.
