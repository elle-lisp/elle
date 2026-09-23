# lir/lower

<!-- audited: 2026-09-23 -->

HIR to LIR lowering: explicit control flow, binding slot allocation, capture cells, and region RC instruction emission.

The LIR types themselves (`LirFunction`, `BasicBlock`, `LirInstr`,
`SpannedInstr`, `Terminator`, `Reg`, `Label`), the emitter, source-location
tracking and the instruction set are described in [../AGENTS.md](../AGENTS.md).
This file covers what the `Lowerer` decides.

## Responsibility

- Lower HIR to explicit control flow (basic blocks, jumps)
- Translate `Binding` references to concrete slot indices
- Emit capture-cell operations for captured and mutated bindings
- Emit region RC instructions (`IncrefRegion`/`DecrefRegion` and the
  value/cell-targeted variants) from the region solver's `RegionInfo`

Does NOT:
- Resolve bindings (that's HIR)
- Decide region assignment, merging or release placement (that's the region
  solver, [infer.rs](../../hir/region/infer.rs)) — the lowerer *emits* from `RegionInfo`
- Execute code (that's VM)

## Key types

| Type | Purpose |
|------|---------|
| `Lowerer` | Main struct that transforms HIR → LIR ([mod.rs](mod.rs)) |
| `BlockLowerContext` | Active block for `break` lowering (block_id, result_reg, result_slot, exit_label) |
| `LoopLowerContext` | Active loop for `Recur` lowering (loop_label, binding_slots, region_id) |

The lowerer reads binding metadata through `&BindingArena` (passed to
`Lowerer::new`), so analysis-phase metadata cannot change during lowering.

## Immutable constant propagation

The lowerer maintains `immutable_values: HashMap<Binding, Value>` mapping
bindings to their compile-time constant values. When `lower_var` encounters
a binding in this map, it emits `ValueConst` instead of `LoadLocal`, avoiding
slot indirection entirely.

Sources of immutable values:
- **Primitives**: seeded by `with_primitive_values` at construction, from the
  analyzer's `primitive_values()`. Covers `+`, `map`, `inc`, etc.
- **User constants**: `try_seed_immutable(binding, init)` seeds an immutable
  binding whose initializer is a literal (Int, Float, Bool, Nil, Keyword,
  Quote) or a reference to another known constant (`hir_const_value`).

Eviction: `lower_bind_value`, `lower_assign` and `lower_letrec` remove a
binding from `immutable_values` when it is re-stored. This handles file-scope
duplicate names where the same Binding identity is reused by a later
destructure.

The map is NOT saved/restored across lambda boundaries, so constants from
the parent scope are visible inside nested lambdas. A captured binding
in `immutable_values` emits `ValueConst` in the lambda body instead of
`LoadCapture` — the capture slot still exists (for the capture mechanism)
but is never read.

## Region instructions

The lowerer decides no lifetime. The region solver assigns each allocation
site its region, merges some regions into one (`merged_parent`, which
`static_slot` reads through `merged_root`), and places each release; the
lowerer emits what `RegionInfo` says. See
[the region model](../../../docs/regions.md) for the memory model.

**Lowerer inputs from `RegionInfo`:**
- `alloc_region[hir_id]` — the region each allocation site is born into.
- `region_data[rid].decref_point` — the HirId after which the lowerer emits
  the region's release.
- `cross_region_refs` — cross-region edges that drive `IncrefRegion`
  emission at the storage site (`emit_increfs_for`).

`with_region_info` ([order.rs](order.rs)) builds two reverse indexes once:
`increfs_by_site` over `cross_region_refs`, and `decrefs_by_decref_point` over
`region_data`. After lowering each HIR node, `emit_decrefs_for`
([regiondecref.rs](regiondecref.rs)) looks that node up and emits one release
per region it finds, in the order `order_releases` sorted them into: holder
before holdee.

Emission is split by kind: [regionemit.rs](regionemit.rs) and
[regionemit/](regionemit/) hold the retains, cell stores and slot choices;
[regiondecref.rs](regiondecref.rs) holds the releases.

## Tail-call ownership

Four mechanisms decide who owns a reference across a tail call. Each one's
argument lives in its doc comment; this is where to find them.

- `tail_arg_is_borrowed` ([control.rs](control.rs)) — a tail-call argument is
  borrowed when any value-producing leaf of it is a captured upvalue (inside a
  lambda), a binding a pattern bound to a borrowed subview of its scrutinee
  (`destructure_alias_bindings`), or a heap constant. It is structural and does
  not read `EscapeInfo`, because escape over-approximates "the env owns it" and
  a mint for an owned escaping argument double-releases across a fiber
  suspend/resume.
- `tail_callee_defers_release` ([control/call/defer.rs](control/call/defer.rs))
  — a per-call callee closure whose release the solver placed at the call is
  stranded by the frame-replacing `TailCall`, so the new activation takes that
  release over (`defer_callee_release`). The decision reads two facts:
  region-locality (the callee's region demises here and its decref is not
  suppressed) and non-escape (`EscapeInfo::lambda_escapes_definition` /
  `binding_escapes_activation`). A stranded recursive callee
  ([docs/impl/selfrec.md](../../../docs/impl/selfrec.md)) and a letrec member the
  body tail-calls (`stranded_member_bindings`) take the narrower question
  `escapes_fiber` alone.
- `open_tail_exit_hoist` ([relocate.rs](relocate.rs)) with `with_tail_exit_hoist`
  ([splice.rs](splice.rs)) — everything the lowerer emits after a `TailCall`
  runs only on the native fall-through, so a release stranded there is RELOCATED
  ahead of the `TailCall`. What the call can still reach keeps its place: the
  regions its callee, operands, result and `deferred_release_slot` channel name,
  and any run that reloads an operand's slot or reads a register defined outside
  it (`hoistable_run`). A destructured leaf argument keeps the exemption only
  where the call passes the slot the region's own release route loads
  (`drop_named_only_arg_exemptions`). The region must also be frame-held
  (`RegionInfo::frame_held_regions`), because a tail callee reaches its captured
  environment too. The argument is
  [docs/impl/region/relocate.md](../../../docs/impl/region/relocate.md).
- `begin_branch_arms` / `seal_arm_hoists` / `open_branch_merge`
  ([relocate.rs](relocate.rs)) — a branch merge inherits the relocation points
  of its arms and of the position the branch was entered at, so a release
  emitted past the merge is emitted there AND replicated ahead of each arm's
  `TailCall`. `self_cancelling_run` ([splice.rs](splice.rs)) is what makes that
  count once per path: a value-routed release nil-stamps the slot it read.
  `emit_decref_for_region` ([regiondecref.rs](regiondecref.rs)) takes the value
  route of the slot the region's binder recorded whenever some point admits the
  region (`replicating_release`; `value_release_slot` names the refusals). The
  argument is [docs/impl/region/replicate.md](../../../docs/impl/region/replicate.md).

## Emit lowering

`lower_emit` ([control.rs](control.rs)) ends the block with `Terminator::Emit`
and opens the resume block, whose first instruction is `LoadResumeValue`. Two
region obligations ride on it:

1. A suspending emit whose payload the body releases nowhere
   (`RegionInfo::borrowed_emit_payloads`) retains the payload before the
   terminator and releases it first thing in the resume block, through a slot of
   its own ([docs/impl/region/park.md](../../../docs/impl/region/park.md)).
2. The resume value arrives uncounted, so the lowerer mints the reference the
   body holds it by (`IncrefValueRegion`) unless the frame's return transfer
   already funds one (`RegionInfo::unfunded_resume_values`;
   [park.md](../../../docs/impl/region/park.md): "A resume
   value crosses counted, or not at all").

## Block/Break lowering

`HirKind::Block` lowers to a result slot + exit label:
1. Allocate `result_reg`, `result_slot` and `exit_label`
2. Push `BlockLowerContext { block_id, result_reg, result_slot, exit_label }`
3. Lower the body and store its result into `result_slot`
4. Pop the context, jump to `exit_label`, and load `result_slot` there

`HirKind::Break` (`lower_break`, [expr/loops.rs](expr/loops.rs)) finds the
target's context in `block_lower_contexts`, lowers the value, stores it into
the target's `result_slot`, jumps to `exit_label`, and starts an unreachable
dead-code block. No new bytecode instructions: a break is `StoreLocal` + `Jump`.

Break emits **no** region instruction of its own, and neither does Block. Every
region the jump affects is anchored by the analysis where the *block's* value is
consumed — at the `Block` node itself when nothing consumes it, which the lowerer
emits after the exit label, so it fires on the break path and the fall-through
path alike. That covers both the value the break carries out and every *other*
release the jump passes over
([docs/impl/region/anchors.md](../../../docs/impl/region/anchors.md)).

The one release the anchor cannot carry is the **breaking iteration's own**: a
region the loop body allocates is minted per iteration, so the block's exit label
would cover whichever value the slot held last. `lower_break` therefore opens a
relocation point at the end of the block it leaves, exactly as a frame-replacing
tail call opens one ahead of its `TailCall`, and a release emitted while that
block is still open is replicated there (`open_break_exit_hoist`,
[docs/impl/region/replicate.md](../../../docs/impl/region/replicate.md)). Do NOT
free the value the break CARRIES at that point — it is exempt, because the block
is about to hand it to its consumer.

## Invariants

1. **Each register assigned exactly once.** SSA form. If you see a register used before definition, lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Emit`, or `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with "unknown binding," the HIR→LIR mapping is incomplete. The key is `Binding` (a `u32` arena index), the value is the `u16` slot index.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside fn bodies, captures, parameters, and env-celled locals are upvalues; they use LoadCapture/StoreCapture. Other locals use LoadLocal/StoreLocal. `value_slot_for` is the one place that address-space choice is re-derived, so a recording site cannot disagree with the allocation. Two sites mint a COMPILED `MakeCaptureCell` held in the binding's own stack slot instead of a `populate_env` env cell:
   - `allocate_compiled_cell_slot` ([emitops.rs](emitops.rs)) mints the **forward cell** of a binding a sibling closure captures before its initializer runs (`BindingInner::compiled_forward_cell`), from the `lower_letrec` and `lower_begin` pre-passes. It registers the binding in `compiled_cell_bindings` as it mints, so neither can happen without the other; the solver pairs the same two writes in `record_compiled_cell`. A binding in that set is not an upvalue: its slot holds the cell, so it reads LoadLocal + LoadCaptureCell.
   - `lower_let` ([binding/let.rs](binding/let.rs)) mints the cell of a captured `let` binding outside any lambda through `emit_alloc_in`, and does not register it. Nothing can capture a `let` binding before its init runs, so this cell is not a forward cell.

5. **Dual address space inside lambdas.** `allocate_slot` returns env-relative indices for env-celled locals (`num_captures + num_locals`) and stack-relative indices for other locals (`num_locals`). Both increment `num_locals` to keep env placeholder slots aligned. The bytecode emitter's `non_cell_local_slot` converts LoadCapture → LoadLocal for non-cell locals. The JIT's `local_slot_to_var` maps stack-relative slots to the JIT variable space. The WASM emitter uses dedicated WASM locals for stack-relative slots.

6. **`capture_params_mask` is set for mutable parameters.** Bit i set means parameter i needs a cell at call time. With immutable-by-default params, only `@`-prefixed params can be mutated, so this mask is typically 0.

7. **`capture_locals_mask` is set for locals that need env cells.** Slot i set means locally-defined variable i (0-indexed from the first local after params) needs a cell because it's captured by a nested closure or mutated via `assign`. The VM env builder (`populate_env`), the JIT prologue, and the WASM env builders all consult it to skip `CaptureCell` allocation for non-captured locals. It is a `CaptureMask` ([src/value/capturemask.rs](../../value/capturemask.rs)), unbounded in width: a local at any index is named precisely, so an uncaptured local beyond slot 63 gets a bare-NIL env slot instead of a dead, leaked cell. (`capture_params_mask` is still a `u64` — functions don't approach 64 parameters, and the params path has no `>=64` fallback to leak through.)

8. **Docstring is threaded from HIR.** `LirFunction.doc` is copied from `HirKind::Lambda.doc` during lowering, then into `TemplateProto.doc`, which `ClosureTemplate::doc()` reads. It is never encoded in bytecode.

## When to modify

- **Adding a new special form**: Add a case in `lower_expr` ([expr.rs](expr.rs)), implement a `lower_your_form` method
- **Changing binding lowering**: Update [binding.rs](binding.rs) and [binding/](binding/)
- **Changing control flow**: Update [control.rs](control.rs) and
  `control/{shortcircuit,matcharms,call}.rs`
- **Changing pattern matching**: Update [pattern.rs](pattern.rs) and `pattern/{ctor,keyed,matching,seq}.rs`
- **Changing region RC emission**: Update [regionemit.rs](regionemit.rs) or [regiondecref.rs](regiondecref.rs); to change *where* a region is released or which regions merge, edit the region solver in [infer.rs](../../hir/region/infer.rs), not the lowerer
- **Changing tail-call ownership**: Update `tail_arg_is_borrowed` ([control.rs](control.rs)) and `tail_callee_defers_release` ([control/call/defer.rs](control/call/defer.rs))
- **Adding new bytecode instructions**: Update [expr.rs](expr.rs), [control.rs](control.rs), [binding.rs](binding.rs), or [lambda.rs](lambda.rs) to emit them

## Common pitfalls

- **Forgetting to allocate slots**: Every binding used in the function must have a slot allocated via `allocate_slot()`
- **Mixing LoadLocal and LoadCapture**: Inside lambdas, upvalues use LoadCapture; locals use LoadLocal
- **Not emitting cell operations**: If a binding needs a cell, emit `MakeCaptureCell` before storing
- **Not propagating spans**: Every emitted instruction should carry the source span from the HIR node
- **Missing a region demise**: After lowering each HIR node, call `emit_decrefs_for(hir_id, …)`, which emits every release the solver placed there. Forgetting to do so leaks regions.
- **Anchoring a release on a path `break` skips**: a release placed at a `decref_point` between a break site and its target block's exit label never runs on the break path. The analysis anchors what it can on the `Block` node; what the loop barrier refuses there, the break's own relocation point replicates (see Block/Break lowering above). Never hand-write a release at the break site — the two mechanisms already cover every path, and a third would double-free
