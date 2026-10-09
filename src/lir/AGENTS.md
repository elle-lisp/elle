# lir

<!-- audited: 2026-10-06 -->

The LIR's two forms, the registers each instruction reads and writes, and the emitter that turns LIR into stack bytecode.

LIR is SSA form over virtual registers and basic blocks. The lowerer builds
it in a working form and freezes each function as it finishes; everything else
reads the frozen form. [lower/AGENTS.md](lower/AGENTS.md) owns how HIR becomes
LIR: slots, capture cells, constants and the region instructions.
[docs/impl/lir.md](../../docs/impl/lir.md) owns the design: the two forms, the
operand proof, heap literals and `LoadSelf`.

## Size

[code/instr.rs](code/instr.rs) is past the 500-line reading budget, so it
carries no audit stamp and is exempt from the audit queue by name
([docs/impl/audit.md](../../docs/impl/audit.md)). The file is one enum,
`InstrRef`, with a doc comment per variant, and Rust gives no way to split one;
bringing it inside the budget means nesting a group of variants into a sub-enum,
which rewrites every exhaustive match in the crate. That is its own change, not
a rider on whatever touches the file next.

A match over the instruction set splits where an enum cannot:
[emit/instr/ops.rs](emit/instr/ops.rs) hands its tail to
[ops/intrinsics.rs](emit/instr/ops/intrinsics.rs), and every other file here
takes the ordinary budget. The encoder lives in [build/](build/mod.rs) and
the decoder in [code/decode.rs](code/decode.rs), each apart from the enum.

## Interface

The working form, which the lowerer builds ([build/](build/mod.rs)):

| Type | Purpose |
|------|---------|
| `LirBuilder` | Builds one unit's functions in a working region it frees when dropped: a stack of functions under construction, each block's nodes in a `RegionVec`, the splices the relocation makes, and `finish_function`, which freezes a function into a `LirOwned` |
| `LirHead` | The header of the function under construction: name, arity, signal, slot and capture counts, capture masks, docstring, origin, region table, merge set and release tables |
| `RegionVec` | A slice that grows in the working region: push, truncate, and insert at an index |
| `Lowerer` | HIR → LIR, answering a `FrozenModule` ([lower/AGENTS.md](lower/AGENTS.md)) |

An instruction, in both forms:

| Type | Purpose |
|------|---------|
| `InstrRef` | One operation: built by the lowerer, encoded by `LirBuilder::emit`, decoded by every reader ([code/instr.rs](code/instr.rs)) |
| `ConstRef` | An immediate constant: nil, the empty list, a bool, a number, a symbol or a keyword |
| `TemplateBytes`, `Slots`, `ConstList` | The borrowed operands an `InstrRef` carries: a `MaterializeConst`'s encoded template, a tail call's stash slots, a `StructRest`'s keys |
| `OperandProof` | What the front end proved about an operation's operands: nothing, or that every one is an integer |
| `Terminator` | How a block exits: `Return`, `Jump`, `Branch`, `Emit`, `Unreachable` |
| `Reg` | Virtual register, `repr(transparent)` over its `u32` |
| `Label` | Basic block identifier |

The frozen form, which every other reader reads ([code/](code/mod.rs)):

| Type | Purpose |
|------|---------|
| `FrozenModule` | The lowerer's product: an entry function and its closures, which `MakeClosure` names by `ClosureId`, each a `LirOwned` |
| `Op` | One opcode byte per `InstrRef` variant |
| `Node`, `BlockRec`, `ConstRec`, `SiteRec` | The plain records a frozen function is made of |
| `LirCode` | The records, tables and header in `Vec`s: `Send`, and serializable |
| `LirOwned` | A `LirCode` plus the `Value`s its `ValueConst` instructions load |
| `LirBody` | The same records in region pages, as a code payload's `lir` field ([code/body.rs](code/body.rs)) |
| `LirView` | The read API over either home: blocks, instructions, terminators, header, tables, sites |
| `SiteRef` | A yield point or a call site: resume IP, live registers, local count |

The emitter, which reads the frozen form:

| Type | Purpose |
|------|---------|
| `Emitter` | LIR → `ClosureCompiled`, that is `(Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)`, writing each nested lambda's payload into the `CodeArena` it was built over ([emit/mod.rs](emit/mod.rs)). `emit_module_with_lambdas` also answers the header over every closure's payload, for the WASM backend's dual compile |
| `YieldPointInfo` | What emission records at a yield point: resume IP, live registers, local count |
| `CallSiteInfo` | What emission records at a call site, for yield-through-call |
| `testkit::LirFixture` | Builds a frozen function by hand, for tests (`#[cfg(test)]`) |

`ConstRef::Nil` and `ConstRef::EmptyList` are distinct constants. Nil is falsy
and the empty list is truthy, and a list ends in the empty list, never in nil.

## Reading LIR

Read a frozen function through its `LirView`, and match its instructions as
`InstrRef`:

```rust
for block in view.blocks() {
    for node in block.nodes() {
        match node.instr() {
            InstrRef::Call { dst, func, args, .. } => { /* args: &[Reg] */ }
            _ => {}
        }
    }
    let term: Terminator = block.terminator();
}
```

A node answers its span, its opcode, the registers it defines and the registers
it uses, without decoding the rest of the instruction. A view is built over
borrowed slices, so a reader neither knows nor cares which `Vec`s back it.

## Register defs and uses

A frozen node answers the registers it reads with one slice, `NodeRef::uses`,
in a fixed order per variant, once per operand position. It answers the
register it writes with `NodeRef::def`. A `TailCall`'s `dst` is not a def:
the call replaces the frame, so only the JIT's native-callee completion path
writes it. `for_each_terminator_use` reports what a `Terminator` reads. The
WASM register allocator and its liveness analysis walk these, and nothing else
answers the question.

`LirBuilder::emit` takes a node's uses from the `InstrRef` in the same fixed
order, so a node answers exactly the registers its instruction names. The
encoder's match is exhaustive, so a new variant cannot be emitted until it says
which operands are uses.

## Building LIR in tests

`testkit::LirFixture` ([testkit.rs](testkit.rs), `#[cfg(test)]`) assembles a
frozen function instruction by instruction, for the unit tests of every reader
of LIR: the emitter, the JIT, the WASM backend, the MLIR and SPIR-V tiers, and
the cross-thread send path. It mirrors `hir::testkit`
([src/hir/testkit.rs](../hir/testkit.rs)), which does the same job for the
front-end passes. A test outside the crate builds through `LirBuilder` itself.

```rust
let func = LirFixture::new(Arity::Exact(1))
    .name("abs")
    .signal(Signal::errors())
    .block(0, &[InstrRef::LoadCaptureRaw { dst: Reg(0), index: 0 }],
           Terminator::Return(Reg(0)))
    .build();
```

The rules the fixture holds:

1. **`block` appends.** Blocks land in call order, and the first one added
   sets `entry`.
2. **Every span is synthetic.** The fixture emits each instruction and the
   terminator with `Span::synthetic()`. A test that needs real spans builds
   through `LirBuilder`.
3. **`build` infers `num_regs`**: one past the highest register id the blocks
   mention — a def, a use, a terminator use, or a `TailCall`'s result register.
   The count is therefore a fact about the instructions rather than a constant
   to maintain by hand.
4. **`num_regs` overrides the inference**, for a test that wants a count the
   instructions do not justify.
5. **`build` freezes.** The fixture emits through a `LirBuilder` over a heap
   of its own and returns the `LirOwned` its `finish_function` answers, which
   is what every reader takes.

The remaining setters — `name`, `signal`, `num_captures`, `num_locals`,
`num_params`, `num_local_params`, `capture_params_mask`, `vararg_kind`,
`closure_id`, `yield_points`, `call_sites` — write the like-named field. A
frozen function's fields are read-only, so a test sets them through the
fixture.

## Data flow

```text
HIR + spans
    │
    ▼
Lowerer (lower/AGENTS.md)
    │
    ▼
LirBuilder: each block's nodes in a RegionVec, in a working region (the working form)
    │
    ▼
finish_function, per function ──► FrozenModule: one LirOwned per function
    │                              (lower frees the working region here)
    ▼
Emitter, reading each function through a LirView
    ├─► simulate the operand stack to place each register
    ├─► emit instruction bytes, then patch jump offsets
    ├─► build the LocationMap from the node spans
    ├─► collect YieldPointInfo at each Emit terminator
    └─► collect CallSiteInfo at each call, in a function that may suspend
    │
    ▼
ClosureCompiled = (Bytecode, Vec<YieldPointInfo>, Vec<CallSiteInfo>)
    │
    ▼
PayloadParts::lambda, at the MakeClosure that builds the lambda
    ├─► locations ← Bytecode.location_map, sorted
    ├─► children ← Bytecode.children, the lambda's own child headers
    └─► lir ← the lambda's frozen records, with its yield points and call
        sites, for the JIT's side exits
    │
    ▼
CodeArena::write ──► a CodePayload in the unit's code region, whose
                     LirBody every reader reaches through
                     ClosureTemplate::lir()
```

The emitter emits blocks in the order the lowerer finished them, and freezing
keeps that order. A merge block is appended after every block that jumps to it,
so the emitter meets each predecessor first; sorting by label number would
break that, because labels are allocated in creation order.

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
   call and each `TailCall`. `PayloadParts::lambda` hands both to the code
   payload's LIR body beside the frozen records, and the JIT reads the
   payload's.

5. **Call sites are recorded only where the function may suspend.**
   `Emitter.current_func_may_suspend`, set from `signal.may_suspend()`, gates
   the recording. A function that can never yield has no `call_sites`.

6. **A block's first emitted predecessor fixes its operand depth.** Every
   other edge into that block must arrive at the same depth. See "Merge
   operand depth" below.

7. **Every `ValueConst` value is in the constant pool.** Emission adds each
   one to the bytecode constant pool, which keeps it alive for as long as the
   code object. A frozen function's `values` table holds the same values, so it
   needs no owner of its own. A debug build checks this after emission.

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

A `TailCall` records a call site too. Its `resume_ip` is where the block after
the tail call starts, the block a native callee falls through to when it
completes. A compiled frame whose tail callee suspends parks there, so the
resume runs that block ([park.md](../../docs/impl/region/park.md)).
`TailCallArrayMut` records none.

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

The enum in [code/instr.rs](code/instr.rs) is the full list, each variant
with its doc comment. These are the ones a reader of lowered code meets most.

| Instruction | What it does |
|-------------|--------------|
| `Const` | Load a `ConstRef` immediate |
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
  closure from the frozen function its code payload carries
- [src/image/](../image/mod.rs) — dumps a payload's `LirBody` with the rest of
  the payload, and verifies its extents at hydration
- [src/value/send/](../value/send/mod.rs) — carries a closure's `LirCode`, and
  its values through the ordinary value walk
