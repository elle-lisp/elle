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
Lowerer (&BindingArena)
    ├─► seed immutable_values for constant bindings (emit ValueConst instead of LoadLocal)
    ├─► allocate slots for bindings (HashMap<Binding, u16>)
    ├─► emit MakeCaptureCell for captured locals (arena.get(b).needs_capture())
    ├─► lower control flow to jumps
    ├─► emit LoadCapture/StoreCapture for upvalues
    ├─► perform escape analysis for scope allocation
    └─► propagate HIR spans to SpannedInstr
    │
    ▼
LirFunction (basic blocks with SpannedInstr)
```

The lowerer reads binding metadata via `&BindingArena` (passed to `Lowerer::new`): `arena.get(b).needs_capture()`, `arena.get(b).name`, `arena.get(b).is_immutable`, etc. The arena reference is immutable during lowering, ensuring analysis-phase metadata cannot be modified.

## Immutable constant propagation

The lowerer maintains `immutable_values: HashMap<Binding, Value>` mapping
bindings to their compile-time constant values. When `lower_var` encounters
a binding in this map, it emits `ValueConst` (LoadConst) instead of
`LoadLocal`, avoiding slot indirection entirely.

Sources of immutable values:
- **Primitives**: Seeded by `with_primitive_values` at construction from
  `Analyzer::primitive_values()`. Covers `+`, `map`, `inc`, etc.
- **User constants**: `try_seed_immutable(binding, init)` seeds immutable
  bindings (let, def) whose initializer is a literal (Int, Float, Bool,
  Nil, Keyword, Quote) or a reference to another known constant.

Eviction: `lower_bind_value` and `lower_assign` evict from `immutable_values`
when a binding is re-stored. This handles file-scope duplicate names where
the same Binding identity is reused by a later destructure.

The map is NOT saved/restored across lambda boundaries, so constants from
the parent scope are visible inside nested lambdas. A captured binding
in `immutable_values` emits `ValueConst` in the lambda body instead of
`LoadCapture` — the capture slot still exists (for the capture mechanism)
but is never read.

## Source location tracking

`SpannedInstr` wraps `LirInstr` with a `Span` for source location tracking:

```rust
pub struct SpannedInstr {
    pub instr: LirInstr,
    pub span: Span,
}
```

The lowerer propagates HIR spans to LIR instructions. The emitter builds a `LocationMap` that maps bytecode offsets to source locations. This map is stored in `Closure.location_map` and used by the VM for error reporting.

## Region instructions

Region allocation uses `IncrefRegion` and `DecrefRegion` instructions
to manage per-region reference counts. The lowerer emits these based
on output from the region solver (`src/hir/regions.rs`); there is no
local escape-analysis pass that gates region instructions. See
`docs/regions.md` for the full memory model.

**Lowerer outputs from `RegionInfo`:**
- `alloc_region[hir_id]` — the region each allocation site is born
  into.
- `region_data[rid].free_at` — the HirId at which the lowerer emits
  the region's compiler-owned `DecrefRegion`.
- `cross_region_refs` — cross-region edges that drive `IncrefRegion`
  emission at the storage site.

**`regions_demising_at(hir_id)`** is the reverse index over
`region_data.free_at`. After lowering each HIR node, the lowerer
iterates `regions_demising_at(hir_id)` and emits one
`DecrefRegion(rid)` per region in that iterator.

There is no escape-vs-local classification. Every allocation gets its
own region; the runtime tracks references via RC. The compiler's only
decision is *where* in the HIR to drop the initial reference, which
is governed by last-use liveness, not by escape analysis.

Other analyses in `escape.rs` (e.g. `body_escapes_heap_values`,
`callee_return_safe`, `tail_arg_is_safe_extended`) are still relevant
to tail-call rotation safety and call-scoped reclamation. They are
not used for region instruction emission.

## Yield as terminator

`Terminator::Yield { value, resume_label }` correctly models that yield suspends execution and resumes in a new block. The lowerer:

1. Emits `Terminator::Yield` to end the current block
2. Creates a new block at `resume_label`
3. Emits `LoadResumeValue` as the first instruction of the resume block

The emitter preserves stack state across the yield boundary via `yield_stack_state`. This ensures intermediate values computed before yield (e.g., the `1` in `(+ 1 (yield 2) 3)`) survive into the resume block.

## Block/Break lowering

`HirKind::Block` lowers to a result register + exit label pattern:
1. Allocate `result_reg` and `exit_label`
2. Push `BlockLowerContext { block_id, result_reg, exit_label, ... }` recording the active region-demise set at entry
3. Lower body, move result to `result_reg`
4. Pop context, jump to `exit_label`, start new block at `exit_label`

`HirKind::Break` lowers to Move + Jump:
1. Find target block's `result_reg` and `exit_label` via `block_lower_contexts`
2. Lower value, move to `result_reg`
3. Emit compensating `DecrefRegion` instructions for each region whose `free_at` lies between the break site and the target block (so the break path fires the same decrefs a fall-through exit would have)
4. Jump to `exit_label`
5. Start unreachable dead-code block

No new bytecode instructions — break compiles to existing Move + Jump + DecrefRegion.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~280 | `Lowerer` struct, context, entry point, `can_scope_allocate_*` analysis |
| `expr.rs` | ~457 | Expression lowering: literals, operators, calls |
| `binding.rs` | ~280 | Binding forms: `let`, `def`, `var`, `fn` |
| `lambda.rs` | ~250 | fn lowering, closure capture, lbox wrapping |
| `control.rs` | ~200 | Control flow: `if`, `begin`, `match` |
| `pattern.rs` | ~1135 | Pattern matching lowering: decision tree walking, constructor tests |
| `access.rs` | ~85 | Access path loading: navigate cons/array/struct to extract values at a path |
| `escape.rs` | ~693 | Escape analysis helpers: `result_is_safe`, `body_contains_dangerous_outward_set`, `body_contains_escaping_break`, `all_break_values_safe` |
| `decision.rs` | ~100 | Decision tree compilation for pattern matching |

## Key instructions

| Instruction | Stack effect | Notes |
|-------------|--------------|-------|
| `ValueConst` | → value | Compile-time constant (from `immutable_values`); used for immutable bindings with literal inits and primitives. GPU-safe for numeric/bool/nil values. |
| `LoadLocal` | → value | Load from stack slot |
| `StoreLocal` | value → value | Store to slot, keep on stack |
| `LoadCapture` | → value | From closure env, auto-unwraps CaptureCell |
| `LoadCaptureRaw` | → lbox | From closure env, preserves lbox (for forwarding) |
| `StoreCapture` | value → | Into closure env, handles lboxes |
| `MakeCaptureCell` | value → lbox | Wrap in CaptureCell |
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
| `StructRest` | struct → struct | Collect all keys not in exclude set into a new immutable struct; variable-length operands: u16 count + count x u16 const_idx |
| `PushParamFrame` | (none) | Push a new parameter binding frame (operand: count u8) |
| `PopParamFrame` | (none) | Pop the current parameter binding frame |
| `IncrefRegion` | (none) | Increment a region's reference count (cross-region reference taken) |
| `DecrefRegion` | (none) | Decrement a region's reference count; free pages when RC hits 0 (sole region-demise opcode) |

## Invariants

1. **Each register assigned exactly once.** SSA form. If you see a register used before definition, lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Yield`, or `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with "unknown binding," the HIR→LIR mapping is incomplete. The key is `Binding` (hashed by `Value::to_bits()`), the value is `u16` slot index.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside fn bodies, captures, parameters, and LBox locals are upvalues; they use LoadCapture/StoreCapture. Non-LBox locals use LoadLocal/StoreLocal.

5. **Dual address space inside lambdas.** `allocate_slot` returns env-relative indices for LBox locals (`num_captures + num_locals`) and stack-relative indices for non-LBox locals (`num_locals`). Both increment `num_locals` to keep env placeholder slots aligned. The bytecode emitter's `non_cell_local_slot` converts LoadCapture → LoadLocal for non-cell locals. The JIT's `local_slot_to_var` maps stack-relative slots to the JIT variable space. The WASM emitter uses dedicated WASM locals for stack-relative slots.

6. **`capture_params_mask` is set for mutable parameters.** Bit i set means parameter i needs lbox wrapping at call time. With immutable-by-default params, only `@`-prefixed params can be mutated, so this mask is typically 0.

7. **`capture_locals_mask` is set for locals that need lboxes.** Bit i set means locally-defined variable i (0-indexed from the first local after params) needs lbox wrapping because it's captured by a nested closure or mutated via `assign`. With immutable-by-default let bindings, only `@`-prefixed bindings can be mutated, so this mask is typically sparser than before. The JIT uses this to skip `CaptureCell` heap allocation for non-captured, non-mutated `let` bindings. The VM interpreter does not use this mask (it lbox-wraps all locals unconditionally). Both masks are limited to 64 entries (`u64`).

8. **Docstring is threaded from HIR.** `LirFunction.doc` is copied from `HirKind::Lambda.doc` during lowering. The emitter preserves it into `Closure.doc` without encoding it in bytecode.

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
- **Not emitting lbox operations**: If a binding needs an lbox, emit `MakeCaptureCell` before storing
- **Not propagating spans**: Every emitted instruction should carry the source span from the HIR node
- **Missing a region demise**: After lowering each HIR node, iterate `regions_demising_at(hir_id)` and emit one `DecrefRegion(rid)` per region. Forgetting to do so leaks regions.
- **Not handling break compensation**: When emitting `break`, emit compensating `DecrefRegion` instructions for each region whose `free_at` lies between the break site and the target block
