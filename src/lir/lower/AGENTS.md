# lir/lower

HIR to LIR lowering: explicit control flow, binding slot allocation, lbox operations, and region RC instruction emission.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `Binding` references to concrete slot indices
- Emit lbox operations for mutable captures
- Emit region RC instructions (`IncrefRegion`/`DecrefRegion` and the
  value/cell-targeted variants) from the region solver's `RegionInfo`

Does NOT:
- Resolve bindings (that's HIR)
- Decide escape or region assignment (that's the region solver,
  `src/hir/region/infer.rs`) — the lowerer has no escape-analysis pass; it only
  *emits* from `RegionInfo`
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
| `BlockLowerContext` | Active block for `break` lowering (block_id, result_reg, result_slot, exit_label) |

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
    ├─► emit region RC instructions from RegionInfo (regionemit.rs)
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
on output from the region solver (`src/hir/region/infer.rs`); there is no
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
is governed by last-use liveness, not by escape analysis. The lowerer
has **no local escape pass** — there is no `escape.rs` and no
`ScopeStats`/`can_scope_allocate_*`; all of `regionemit.rs`'s emission
reads the solver's `RegionInfo` directly.

There are two surviving tail-call ownership predicates. One reads the
authoritative escape analysis (`EscapeInfo`, `src/hir/escape.rs`); the
other is structural ownership-location, NOT escape:

- `control.rs::tail_arg_is_borrowed` — a tail-call argument is borrowed
  (the env owns the capture-incref, so the frame has no transferable
  owning reference) iff its binding is a captured upvalue
  (`upvalue_bindings`, through `functionalize`'s `DerefCell`). This is
  **structural** and does NOT read `EscapeInfo`: escape over-approximates
  "the env owns it" (a born-here value that flows to a tail escapes but is
  owned), and minting for those owned-escaping args double-releases across
  a fiber suspend/resume — a phantom `DecrefRegion`/UAF (witnessed on
  `contracts.lisp`). The env-ownership fact is structural lexical capture.
- `control/call.rs::tail_callee_defers_release` — the per-call callee closure
  whose `DecrefRegion` the solver placed at this node is stranded as
  dead code by the `TailCall`; setting `defer_callee_release` makes the runtime
  supply that decref. Two facts: region-locality (the callee has an owned
  per-call region demising here — `decrefs_by_decref_point` minus
  `suppressed_decref_regions`, which `EscapeInfo` cannot express) AND
  non-escape (`EscapeInfo::lambda_escapes_definition`/
  `binding_escapes_activation` — the escape half, replacing the old
  region-level proxy). A **stranded recursive** callee takes a narrower
  escape question, `escapes_fiber` alone, because this deferral is its
  region's only release channel: store/capture are containment, and the
  return facet is funded by the callee's own return mint, which precedes
  the deferred decref (docs/impl/selfrec.md). A letrec **member** the body
  tail-calls takes the same narrower question through
  `stranded_member_bindings`: a sibling captures it, so its demise lands at
  the letrec's scope end and region-locality never sees it, while the
  relocation must leave that release alone — the call is about to enter the
  closure it would free (docs/impl/region/mechanism.md § "What the exemption
  keeps, a channel must still run").
- `emitops.rs::open_tail_exit_hoist` / `with_tail_exit_hoist` — the callee's
  region is not the only one stranded past a `TailCall`: everything the
  lowerer emits after it runs only on the native fall-through, so a
  parameter used only inside a closure the body builds, a parameter used
  nowhere, or an env cell has its release emitted where a closure callee
  never arrives. The wrapper RELOCATES that already-emitted release ahead of
  the `TailCall` (pinned by `tests::release`'s two placement tests). The
  relocation does NOT waive the count argument — on the closure path the
  release did not run before and now does — so two gates apply. What may not
  move is what the call can still reach: the regions its callee, operands,
  result and `deferred_release_slot` channel name, plus any run that reloads
  an operand's slot or reads a register defined outside it
  (`hoistable_run`). An operand names its VALUE, not its syntax — the walk
  stops at a `Call`/`Lambda` and records that node's own region, then closes
  the set under the three alias relations, so a region an inner call merely
  used is not exempt while one its result may live inside still is
  (docs/impl/region/mechanism.md § "What an operand names is its VALUE").
  And the region must be sole-frame-held
  (`RegionInfo::sole_frame_held_regions`), because a tail callee also reaches
  its CAPTURED environment, which no argument names. A region escaping by the
  RETURN facet alone (`RegionInfo::return_frame_held_regions`) is admitted at a
  point whose callee captures one of its holders
  (`TailCalleeFacts::capture_funded`): the caller's reference is minted
  by the callee's own `Return`, after the relocated release, and that captured
  edge is the count holding the region off zero in between.
- `emitops.rs::seal_arm_hoists` / `open_branch_merge` — an `if`/`cond`/`match`
  merge is reached only through arms the lowerer closes one at a time, so it
  INHERITS their relocation points and a release emitted past the merge is
  emitted there AND replicated ahead of each arm's `TailCall`. What makes
  that count once per path is `self_cancelling_run`: a value-routed release
  nil-stamps the slot it read, so the copy a path reaches second loads `nil`
  and no-ops. A run without that stamp (`DecrefRegion` by id,
  `DecrefCellRegion`, the transfer adopt) keeps the baseline, and every other
  block boundary clears the points — a replica in a point the emission
  position is unreachable from is a release added where none was owed
  (docs/impl/region/mechanism.md § "A release past a frame-replacing tail
  call is not a release").

## Yield as terminator

`Terminator::Yield { value, resume_label }` correctly models that yield suspends execution and resumes in a new block. The lowerer:

1. Emits `Terminator::Yield` to end the current block
2. Creates a new block at `resume_label`
3. Emits `LoadResumeValue` as the first instruction of the resume block

The emitter preserves stack state across the yield boundary via `yield_stack_state`. This ensures intermediate values computed before yield (e.g., the `1` in `(+ 1 (yield 2) 3)`) survive into the resume block.

## Block/Break lowering

`HirKind::Block` lowers to a result register + exit label pattern:
1. Allocate `result_reg` and `exit_label`
2. Push `BlockLowerContext { block_id, result_reg, result_slot, exit_label }`
3. Lower body, move result to `result_reg`
4. Pop context, jump to `exit_label`, start new block at `exit_label`

`HirKind::Break` lowers to Move + Jump:
1. Find target block's `result_reg` and `exit_label` via `block_lower_contexts`
2. Lower value, move to `result_reg`
3. Jump to `exit_label`
4. Start unreachable dead-code block

No new bytecode instructions — break compiles to existing Move + Jump.

Break emits **no** region instruction of its own, and neither does Block. Every
region the jump affects is anchored by the analysis where the *block's* value is
consumed — at the `Block` node itself when nothing consumes it, which the lowerer
emits after the exit label, so it fires on the break path and the fall-through
path alike. That covers both faces: the value the break carries out
([mechanism.md](../../../docs/impl/region/mechanism.md) § "`break` transfers its
value; it does not consume it") and every *other* release the jump passes over
(§ "A release the break jumps over is not a release"). Do not add a compensating
release at the break site — for the broken value it would free what the block is
about to hand its consumer, and for the rest there is nothing left to free.

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

7. **`capture_locals_mask` is set for locals that need lboxes.** Slot i set means locally-defined variable i (0-indexed from the first local after params) needs lbox wrapping because it's captured by a nested closure or mutated via `assign`. With immutable-by-default let bindings, only `@`-prefixed bindings can be mutated, so this mask is typically sparse. The VM env builder (`populate_env`), the JIT prologue, and the WASM env builders all consult it to skip `CaptureCell` allocation for non-captured locals. It is a `CaptureMask` (`src/value/capturemask.rs`), unbounded in width: a local at any index is named precisely, so an uncaptured local beyond slot 63 gets a bare-NIL env slot instead of a dead, leaked cell. (`capture_params_mask` is still a `u64` — functions don't approach 64 parameters, and the params path has no `>=64` fallback to leak through.)

8. **Docstring is threaded from HIR.** `LirFunction.doc` is copied from `HirKind::Lambda.doc` during lowering. The emitter preserves it into `Closure.doc` without encoding it in bytecode.

## When to modify

- **Adding a new special form**: Add a case in `expr.rs::lower_expr`, implement `lower_your_form` method
- **Changing binding lowering**: Update `binding.rs`
- **Changing control flow**: Update `control.rs`
- **Changing pattern matching**: Update `pattern.rs` and `pattern/{keyed,matching,seq}.rs`
- **Changing region RC emission**: Update `regionemit.rs` (it reads the solver's `RegionInfo`); to change *what* is escaping or *where* a region is dropped, edit the region solver in `src/hir/region/infer.rs`, not the lowerer
- **Changing tail-call ownership**: Update `control.rs::tail_arg_is_borrowed` and `control/call.rs::tail_callee_defers_release`
- **Adding new bytecode instructions**: Update `expr.rs`, `control.rs`, `binding.rs`, or `lambda.rs` to emit them

## Common pitfalls

- **Forgetting to allocate slots**: Every binding used in the function must have a slot allocated via `allocate_slot()`
- **Mixing LoadLocal and LoadCapture**: Inside lambdas, upvalues use LoadCapture; locals use LoadLocal
- **Not emitting lbox operations**: If a binding needs an lbox, emit `MakeCaptureCell` before storing
- **Not propagating spans**: Every emitted instruction should carry the source span from the HIR node
- **Missing a region demise**: After lowering each HIR node, iterate `regions_demising_at(hir_id)` and emit one `DecrefRegion(rid)` per region. Forgetting to do so leaks regions.
- **Anchoring a release on a path `break` skips**: a release placed at a `decref_point` between a break site and its target block's exit label never runs on the break path. Both the broken value and every other region in that window are anchored on the `Block` node by the analysis (see Block/Break lowering above) — never patched at the break site
