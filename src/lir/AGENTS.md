# lir

<!-- audited: 2026-09-23 -->

The LIR types, the registers each instruction reads and writes, and the emitter that turns LIR into stack bytecode.

LIR is SSA form over virtual registers and basic blocks.
[lower/AGENTS.md](lower/AGENTS.md) owns how HIR becomes LIR: slots, capture
cells, constants and the region instructions.
[docs/impl/lir.md](../../docs/impl/lir.md) owns the design: the operand proof,
heap literals and `LoadSelf`.

## Size

[types/instr.rs](types/instr.rs) is past the 500-line reading budget and
carries no audit stamp, so it sits in the queue. The file is one enum, and Rust
gives no way to split one; bringing it inside the budget means nesting a group
of variants into a sub-enum, which rewrites every exhaustive match in the
crate. That is its own change, not a rider on whatever touches the file next.

A match over the instruction set splits where an enum cannot:
[emit/instr/ops.rs](emit/instr/ops.rs) hands its tail to
[ops/intrinsics.rs](emit/instr/ops/intrinsics.rs), and every other file here
takes the ordinary budget.

## Interface

| Type | Purpose |
|------|---------|
| `LirFunction` | Compilation unit: blocks, constants, slot counts, capture masks, signal, docstring, origin, region tables, yield-point and call-site metadata ([types/func.rs](types/func.rs)) |
| `BasicBlock` | Instructions + terminator |
| `LirInstr` | Individual operation ([types/instr.rs](types/instr.rs)) |
| `OperandProof` | What the front end proved about an operation's operands: nothing, or that every one is an integer |
| `SpannedInstr` | `LirInstr` + `Span` for source tracking |
| `SpannedTerminator` | `Terminator` + `Span` for source tracking |
| `Terminator` | How a block exits: `Return`, `Jump`, `Branch`, `Emit`, `Unreachable` |
| `LirConst` | An immediate constant: nil, the empty list, a bool, number, string, symbol or keyword |
| `Reg` | Virtual register |
| `Label` | Basic block identifier |
| `YieldPointInfo` | Metadata for a yield point: resume IP, live registers, local count |
| `CallSiteInfo` | Metadata for a call site: resume IP, live registers, local count (for yield-through-call) |
| `Lowerer` | HIR → LIR ([lower/AGENTS.md](lower/AGENTS.md)) |
| `Emitter` | LIR → `ClosureCompiled`, that is `(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)` ([emit/mod.rs](emit/mod.rs)) |
| `for_each_def` / `for_each_use` / `for_each_terminator_use` | The registers an instruction or terminator writes and reads |
| `testkit::LirFixture` | Builds a `LirFunction` by hand, for tests (`#[cfg(test)]`) |

`LirConst::Nil` and `LirConst::EmptyList` are distinct constants. Nil is falsy
and the empty list is truthy, and a list ends in the empty list, never in nil.

## Register defs and uses

`for_each_def`, `for_each_use` and `for_each_terminator_use`
([types/regs.rs](types/regs.rs)) report the registers an instruction writes and
reads. They are the single answer to that question for the whole crate: the
WASM register allocator and its liveness analysis both walk them, and so does
the test fixture below when it infers a register count. A new `LirInstr`
variant must be added to all three — the matches are exhaustive, so the
compiler names the omission.

## Building LIR in tests

`testkit::LirFixture` ([testkit.rs](testkit.rs), `#[cfg(test)]`) assembles a
`LirFunction` directly, for the unit tests of every consumer of LIR: the
emitter, the JIT, the WASM backend, the MLIR and SPIR-V tiers, and the
cross-thread send path. It mirrors `hir::testkit`
([src/hir/testkit.rs](../hir/testkit.rs)), which does the same job for the
front-end passes.

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
field. Fields with no setter are public on the built `LirFunction`: set them on
the result, as the JIT's arity and `vararg_kind` tests do.

## Data flow

```text
HIR + spans
    │
    ▼
Lowerer (lower/AGENTS.md)
    │
    ▼
LirFunction (basic blocks of SpannedInstr)
    │
    ▼
Emitter
    ├─► simulate the operand stack to place each register
    ├─► emit instruction bytes, then patch jump offsets
    ├─► build the LocationMap from SpannedInstr spans
    ├─► collect YieldPointInfo at each Emit terminator
    └─► collect CallSiteInfo at each call, in a function that may suspend
    │
    ▼
ClosureCompiled = (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)
    │
    ▼
TemplateProto::nested_lambda
    ├─► location_map ← Bytecode.location_map
    └─► lir_function ← a copy of the LirFunction, with yield_points and
        call_sites filled in, for the JIT's side exits
```

The emitter emits blocks in the order the lowerer appended them. A merge block
is appended after every block that jumps to it, so the emitter meets each
predecessor first; sorting by label number would break that, because labels
are allocated in creation order.

## Invariants

1. **Each register is assigned exactly once.** SSA form. A register used
   before its definition means lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Emit`
   or `Unreachable`. No fall-through. A tail call is an instruction
   (`TailCall`, `TailCallArrayMut`), not a terminator.

3. **Emit is a block terminator, not an instruction.**
   `Terminator::Emit { signal, value, resume_label }` ends the block, and the
   resume block opens with `LoadResumeValue`, which takes the value passed to
   `fiber/resume`. The emitter carries the stack simulation across the emit
   through `yield_stack_state`, so a value computed before the emit, such as
   the `1` in `(+ 1 (emit :yield 2) 3)`, survives into the resume block.

4. **Yield and call-site metadata come from emission.** The emitter records a
   `YieldPointInfo` at each `Terminator::Emit` and a `CallSiteInfo` at each
   call. `TemplateProto::nested_lambda`
   ([src/value/closure/proto.rs](../value/closure/proto.rs)) writes both into
   the template's copy of the `LirFunction`, which is what the JIT reads.

5. **Call sites are recorded only where the function may suspend.**
   `Emitter.current_func_may_suspend`, set from `signal.may_suspend()`, gates
   the recording. A function that can never yield has no `call_sites`.

6. **A block's first emitted predecessor fixes its operand depth.** Every
   other edge into that block must arrive at the same depth. See "Merge
   operand depth" below.

## Yield and call-site metadata

`YieldPointInfo`:
- `resume_ip` — the bytecode offset to resume at, after the emit opcode.
- `stack_regs` — the registers on the operand stack at the emit, bottom to top.
- `num_locals` — the local slots below them.

The JIT spills the locals, then these registers, and calls the emit runtime
helper.

`CallSiteInfo`:
- `resume_ip` — the bytecode offset after the call instruction, where the
  interpreter resumes if the callee yields.
- `stack_regs` — the registers on the operand stack after the callee and
  arguments are popped and before the result is pushed.
- `num_locals` — the local slots below them.

That is the interpreter's stack when a yield propagates through a call. The JIT
builds the caller's `SuspendedFrame` from it when a callee yields.

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

A forward edge meets the rule with `pop_trailing_orphans_to`. An orphan is a
stack cell that no register's canonical position names — the residue
`ensure_on_top` leaves when it copies a value up with `DupN` and the copy is
then consumed. Orphans are dead, so popping them is free.

But the pops are per-edge, and only `Terminator::Jump` performs them, so they
must be **bounded by the target's already-fixed depth**. Popping past it
splits the paths: the branch edge into the merge leaves the orphan, the jump
edge removes it, and the merge's successors — which inherited the branch's
simulation — pop it a second time on the path that already did. Two pops, one
value, and the second one lands in the reserved local region. This is why
`Terminator::Jump` trims a forward edge only down to the target's recorded
depth (`yield_stack_state`) rather than to the first live value.

### A back edge restores the depth its target was fixed at

The orphan trim cannot meet the rule at a back edge. It stops at the first live
cell, so an orphan the block left *under* a live one survives the jump.
Straight-line code absorbs that residue; a loop accumulates it. The header runs
again a cell or two deeper each time, and the activation's operand stack grows
for as long as the loop does — an unbounded leak that no region gauge sees,
because the cells are a `Fiber`'s own `SmallVec` rather than heap objects.

A back edge is the one edge that may trim to the target's depth outright, so
`Terminator::Jump` uses `pop_to` there: the target was emitted before this edge
existed, its simulation names only cells below that depth, and it therefore
reads nothing this block pushed. Every surplus cell goes, orphan and live alike,
and the header meets the same stack shape on every pass.

A forward merge keeps the orphan trim, because the same argument does not hold
for it. Its target is still ahead of the cursor, and a later edge's own result
may sit above an orphan — trimming to the depth would drop the result and keep
the orphan.

## Key instructions

The enum in [types/instr.rs](types/instr.rs) is the full list, each variant
with its doc comment. These are the ones a reader of lowered code meets most.

| Instruction | What it does |
|-------------|--------------|
| `Const` | Load a `LirConst` immediate |
| `ValueConst` | Load a compile-time `Value`: a primitive, or an immutable binding with a literal initializer |
| `MaterializeConst` | Allocate a heap literal from its template into its own region |
| `LoadLocal` / `StoreLocal` | Read or write a stack slot |
| `LoadCapture` | Read a closure-environment slot, unwrapping a capture cell |
| `LoadCaptureRaw` | Read a closure-environment slot without unwrapping, to forward the cell to a nested closure |
| `StoreCapture` | Write a closure-environment slot, through its cell when it holds one |
| `MakeCaptureCell` / `LoadCaptureCell` / `StoreCaptureCell` | Build, read and write a capture cell held in a register |
| `MakeClosure` | Build a closure from the lambda its `ClosureId` names, and its captures |
| `LoadSelf` | Load the executing closure |
| `Call` / `SuspendingCall` / `TailCall` | Call a function |
| `LoadResumeValue` | First instruction of an emit's resume block |
| `FirstOrNil` / `RestOrNil` | First or rest of a pair; nil or the empty list when the value is not a pair |
| `ArrayMutRefOrNil` | Array element at an immediate index; nil when out of bounds or not an array |
| `StructGetOrNil` | Struct field by constant key; nil when missing or not a struct |
| `IsArray` / `IsArrayMut` / `IsStruct` / `IsStructMut` | Type checks for pattern matching |
| `ArrayMutLen` | Array length, for pattern matching |
| `PushParamFrame` / `PopParamFrame` | Push a frame of (parameter, value) register pairs for `parameterize`, and pop it |
| `IncrefRegion` / `DecrefRegion` | Retain or release a region by its static slot |
| `IncrefValueRegion` / `DecrefValueRegion` / `DecrefCellRegion` | Retain or release the region a value or a capture cell lives in |

[lower/AGENTS.md](lower/AGENTS.md) says where the lowerer emits the region
instructions, and [docs/regions.md](../../docs/regions.md) says what they
count.

## Dependents

- [src/pipeline/](../pipeline/) — runs the `Lowerer` and the `Emitter`
- [src/vm/](../vm/) — executes the emitted bytecode
- [src/jit/](../jit/), [src/wasm/](../wasm/) and the MLIR tier — compile a
  closure from the `LirFunction` its template keeps
