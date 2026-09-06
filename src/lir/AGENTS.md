# lir

<!-- audited: 2026-09-06 -->

Low-level Intermediate Representation. SSA form with virtual registers
and basic blocks. Architecture-independent but close to target.

## Size

`types/instr.rs` is past the 500-line reading budget and carries no audit stamp,
so it sits in the queue. The file is one enum, and Rust gives no way to split
one; bringing it inside the budget means nesting a group of variants into a
sub-enum, which rewrites every exhaustive match in the crate. That is its own
change, not a rider on whatever touches the file next.

A match over the instruction set splits where an enum cannot: `emit/instr/ops.rs`
hands its tail to `ops/intrinsics.rs`, and every other file here takes the
ordinary budget.

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
| `OperandProof` | What the front end proved about an operation's operands: nothing, or that every one is an integer. See [lir.md](../../docs/impl/lir.md) |
| `SpannedInstr` | `LirInstr` + `Span` for source tracking |
| `SpannedTerminator` | `Terminator` + `Span` for source tracking |
| `Terminator` | How block exits: `Return`, `Jump`, `Branch`, `Emit` |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `YieldPointInfo` | Metadata for a yield point: resume IP and live registers |
| `CallSiteInfo` | Metadata for a call site: resume IP and live registers (for yield-through-call) |
| `Lowerer` | HIR → LIR |
| `ScopeStats` | Compile-time scope allocation statistics |
| `Emitter` | LIR → (Bytecode, yield_points, call_sites) |
| `for_each_def` / `for_each_use` / `for_each_terminator_use` | The registers an instruction or terminator writes and reads |
| `testkit::LirFixture` | Builds a `LirFunction` by hand, for tests (`#[cfg(test)]`) |

## Register defs and uses

`for_each_def`, `for_each_use` and `for_each_terminator_use` (`types/regs.rs`)
report the registers an instruction writes and reads. They are the single
answer to that question for the whole crate: the WASM register allocator and
its liveness analysis both walk them, and so does the test fixture below when
it infers a register count. A new `LirInstr` variant must be added to all
three — the matches are exhaustive, so the compiler names the omission.

## Building LIR in tests

`testkit::LirFixture` (`src/lir/testkit.rs`, `#[cfg(test)]`) assembles a
`LirFunction` directly, for the unit tests of every consumer of LIR: the
emitter, the JIT, the WASM backend, the MLIR and SPIR-V tiers, and the
cross-thread send path. It mirrors `hir::testkit` (`src/hir/testkit.rs`),
which does the same job for the front-end passes.

```rust
let func = LirFixture::new(Arity::Exact(1))
    .name("abs")
    .signal(Signal::errors())
    .block(0, vec![LirInstr::LoadCaptureRaw { dst: Reg(0), index: 0 }],
           Terminator::Return(Reg(0)))
    .build();
```

The rules the fixture holds:

1. **`block` appends.** Blocks land in call order, and the first one added
   sets `entry`.
2. **Every span is synthetic.** The fixture wraps each `LirInstr` in a
   `SpannedInstr` and the terminator in a `SpannedTerminator`, both with
   `Span::synthetic()`. A test that needs real spans builds its blocks itself.
3. **`build` infers `num_regs`**: one past the highest register id the blocks
   mention — a def, a use, a terminator use, or a `TailCall`'s result register.
   The count is therefore a fact about the instructions rather than a constant
   to maintain by hand.
4. **`num_regs` overrides the inference**, for a test that wants a count the
   instructions do not justify.

The remaining setters — `name`, `signal`, `num_captures`, `num_locals`,
`num_params`, `closure_id`, `yield_points`, `call_sites` — write the like-named
field.
Fields with no setter are public on the built `LirFunction`: set them on the
result, as the JIT's arity and `vararg_kind` tests do.

## Data flow

```
HIR + spans
    │
    ▼
Lowerer (&BindingArena)
    ├─► seed immutable_values for constant bindings (emit ValueConst instead of LoadLocal)
    ├─► allocate slots for bindings (HashMap<Binding, u16>)
    ├─► emit MakeCaptureCell for captured locals (arena.get(b).needs_capture();
    │   top level, plus compiled-cell letrec bindings in-lambda — invariant 6)
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
`arena.get(b).needs_capture()`, `arena.get(b).name`, etc.

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

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Emit`,
    or `Unreachable`. No fall-through.

3. **`binding_to_slot` maps all accessed bindings.** If lowering fails with
    "unknown binding," the HIR→LIR mapping is incomplete. The key is `Binding`
    (hashed by `Value::to_bits()`), the value is `u16` slot index.

4. **`upvalue_bindings` tracks what uses LoadCapture.** Inside fn bodies,
    captures and parameters are upvalues; they use LoadCapture, not LoadLocal.

5. **`capture_params_mask` is set for mutable parameters.** Bit i set means
     parameter i needs lbox wrapping at call time. With immutable-by-default
     params, only `@`-prefixed params can be mutated, so this mask is typically 0.

6. **`capture_locals_mask` is set for locals that need lboxes.** Slot i set means
     locally-defined variable i (0-indexed from the first local after params)
     needs lbox wrapping because it's captured by a nested closure or mutated
     via `assign`. With immutable-by-default let bindings, only `@`-prefixed
     bindings can be mutated, so this mask is typically sparse. The VM env
     builder, the JIT prologue, and the WASM env builders all consult it to skip
     `CaptureCell` allocation for non-captured locals. It is a `CaptureMask`
     (`src/value/capturemask.rs`), **unbounded in width**: a local at any index
     is named precisely, so an uncaptured local beyond slot 63 gets a bare-NIL
     env slot, never a dead, leaked cell. (`capture_params_mask` stays a `u64`;
     its path has no `>=64` fallback and functions never approach 64 params.)
     One captured shape is deliberately NOT mask-set: a `letrec` binding whose
     forward cell is COMPILED (`BindingInner::letrec_compiled_cell` — immutable,
     never mutated, lambda-initialized, in every position including inside a
     lambda). Its `MakeCaptureCell` value lives in a plain stack slot
     (`allocate_slot_routed`), giving the cell a static region slot the
     closure-cycle merge can collapse; the env must not mint a shadow cell.

7. **Emit is a block terminator, not an instruction.** `Terminator::Emit { signal: SignalBits, value: Reg, resume_label: Label }`
    splits the block: the current block ends with emit, and a new resume block
    begins. The resume block starts with `LoadResumeValue` to capture the value
    passed to `fiber/resume`.

8. **Docstring and syntax are threaded from HIR.** `LirFunction.doc` and
      `LirFunction.syntax` are copied from `HirKind::Lambda.doc` and
      `HirKind::Lambda.syntax` during lowering. The emitter preserves both
      into `Closure.doc` and `Closure.syntax` without encoding them in bytecode.

9. **Emit point metadata is collected during emission.** `Emitter::emit()`
     returns `(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)`. The caller
     must attach these to `LirFunction.yield_points` and `LirFunction.call_sites`
     before storing the function on a `Closure`. The JIT reads this metadata
     to generate side-exit code.

10. **Call site metadata is only populated for may_suspend functions.**
     `Emitter.current_func_may_suspend` gates call site recording. For
     non-suspending functions, `call_sites` is empty. This avoids overhead
     for silent functions that can never yield.

11. **A block's first emitted predecessor fixes its operand depth.** Every
     other edge into that block must arrive at the same depth. See "Merge
     operand depth" below.

## Merge operand depth

The VM addresses local `n` as `frame_base + n` on the operand stack, so the
entry block reserves `num_locals` positions and operands stack above them
(`Emitter::emit_block`). A path that pops one operand too many falls through
that floor and destroys a live local; the damage shows up much later, as a
`LoadLocal` of a high slot indexing past the end of the stack.

The emitter simulates the operand stack per block. A block inherits its
starting simulation from the first predecessor that reaches it
(`yield_stack_state`, first writer wins), because the simulation cannot
reconcile two different incoming shapes. That makes one rule mandatory:

> **The first predecessor emitted fixes the merge block's operand depth, and
> every later edge into that block must leave exactly that depth.**

`pop_trailing_orphans_to` is the only tool the emitter has for meeting the rule.
An orphan is a stack cell that no register's canonical position names — the
residue `ensure_on_top` leaves when it copies a value up with `DupN` and the
copy is then consumed. Orphans are dead, so popping them is free; popping
them is also what keeps a loop body from growing the stack by one cell per
iteration.

But the pops are per-edge, and only `Terminator::Jump` performs them, so they
must be **bounded by the target's already-fixed depth**. Popping past it
splits the paths: the branch edge into the merge leaves the orphan, the jump
edge removes it, and the merge's successors — which inherited the branch's
simulation — pop it a second time on the path that already did. Two pops, one
value, and the second one lands in the reserved local region. This is why
`Terminator::Jump` trims only down to the target's recorded depth
(`yield_stack_state`, or `block_entry_depth` for a back edge into a block
already emitted) rather than to the first live value.

## Key instructions

| Instruction | Stack effect | Notes |
|-------------|--------------|-------|
| `ValueConst` | → value | Compile-time constant from `immutable_values` (primitives + immutable literal bindings). GPU-safe for numeric/bool/nil values. |
| `LoadLocal` | → value | Load from stack slot |
| `StoreLocal` | value → value | Store to slot, keep on stack |
| `LoadCapture` | → value | From closure env, auto-unwraps CaptureCell |
| `LoadCaptureRaw` | → lbox | From closure env, preserves lbox (for forwarding) |
| `StoreCapture` | value → | Into closure env, handles lboxes |
| `MakeCaptureCell` | value → lbox | Wrap in CaptureCell |
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
| `IncrefRegion` | (none) | Increment a region's reference count (cross-region reference taken) |
| `DecrefRegion` | (none) | Decrement a region's reference count; free pages when RC hits 0 (sole region-demise opcode) |

## Emit and Call Site Metadata

The emitter collects two types of metadata during bytecode emission:

### YieldPointInfo

Recorded when a `Terminator::Emit` is emitted:
- `resume_ip: usize` — Bytecode offset to resume at (the instruction after the Emit opcode)
- `stack_regs: Vec<Reg>` — Virtual registers on the operand stack at emit time, bottom-to-top

The JIT uses this to spill live registers to a stack slot and call the emit runtime helper.

### CallSiteInfo

Recorded when a `LirInstr::Call` is emitted in a function where `signal.may_suspend()`:
- `resume_ip: usize` — Bytecode offset after the Call instruction (where the interpreter resumes if the callee yields)
- `stack_regs: Vec<Reg>` — Virtual registers on the operand stack after popping func/args but before pushing the result

This matches the interpreter's stack state when yield propagates through a call. The JIT uses this to build the caller's `SuspendedFrame` when a callee yields (yield-through-call).

## Allocation regions

`IncrefRegion` and `DecrefRegion` are the only region-lifecycle
bytecodes. The lowerer emits them based on output from the region
solver (`src/hir/region/infer.rs`), not a local escape analysis pass.
`DecrefRegion` is the sole region-demise opcode — there is no
`FreeRegion`.

The solver produces `RegionInfo` containing `alloc_region` (which
region each allocation site is born into) and, per region,
`RegionData { free_at: HirId, ... }` — the program point at which the
compiler emits the region's `DecrefRegion`. Plus `cross_region_refs`
for the cross-region edges that drive `IncrefRegion` emission.

At lowering time the lowerer reverse-indexes `region_data` to ask
"which regions demise at this HirId?" and emits one `DecrefRegion(rid)`
per region in that set after lowering the HIR node. Region demise is
keyed per-`HirId` by the solver's `free_at` point, not by lexical
scope.

`break` emits compensating `DecrefRegion` instructions for each
region whose `free_at` lies between the break site and the target
block. The `BlockLowerContext` records the relevant `free_at` set at
entry so the break path can fire the same decrefs that a fall-through
exit would have.

**Compile-time scope stats** (`ScopeStats`): The lowerer counts how many
scopes were analyzed, how many qualified for scope allocation, and the
first-failing condition for each rejected scope (captured, suspends,
unsafe-result, outward-set, break). Access via `lowerer.scope_stats()`
after `lower()` completes. Pass `--stats` to the elle CLI to print the
aggregated stats to stderr on program exit (alongside JIT stats).

## Emit as terminator

`Terminator::Emit { signal, value, resume_label }` correctly models that emit
suspends execution and resumes in a new block. The lowerer:

1. Emits `Terminator::Emit` to end the current block
2. Creates a new block at `resume_label`
3. Emits `LoadResumeValue` as the first instruction of the resume block

A suspending emit whose payload the body releases nowhere
(`RegionInfo::borrowed_emit_payloads`) wraps that with the park's borrow mint: an
`IncrefValueRegion` before the terminator and a `DecrefValueRegion` first in the
resume block, with a copy parked in a local slot of its own since the value
register is consumed by the `Emit`. That gives a fiber body one reference of every
value it yields, which is what a discarded fiber's discharge releases
(docs/impl/region/owner.md § "Park/unpark symmetry").

A **dynamic** emit has no `Emit` terminator to wrap — its first argument is not a
literal keyword set, so it lowers as an ordinary call — and `lower_call` carries the
same obligation there. In non-tail position it takes the mint at the payload
argument and releases it after the call, which is where the resume lands; in tail
position the borrowed-argument retain already is that reference (docs/impl/region/
owner.md § "What yields is the emit OPERATION, not the `Emit` node").

The emitter preserves stack state across the emit boundary via
`yield_stack_state`. This ensures intermediate values computed before emit
(e.g., the `1` in `(+ 1 (emit :yield 2) 3)`) survive into the resume block.

**Emit point metadata:** When the emitter encounters a `Terminator::Emit`,
it records a `YieldPointInfo` containing:
- `resume_ip`: Bytecode offset to resume at (the instruction after Emit)
- `stack_regs`: Virtual registers on the operand stack at emit time

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
## Constants

`LirConst` represents compile-time constants. Note: `LirConst::Nil` and
`LirConst::EmptyList` are distinct. Nil is falsy, EmptyList is truthy. Lists
terminate with EmptyList, not Nil.
