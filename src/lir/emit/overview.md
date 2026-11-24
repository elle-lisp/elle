# The emitter

<!-- audited: 2026-11-21 -->

How the emitter turns frozen LIR into stack bytecode, the sites it records for the JIT, and the operand depth each edge leaves.

The emitter reads each function through a `LirView`
([the LIR module](../overview.md)) and writes the bytecode
[the bytecode design](../../../docs/impl/bytecode.md) describes.
[The LIR design](../../../docs/impl/lir.md) says where the result goes: the
code payload `PayloadParts::lambda` writes at the `MakeClosure` that builds a
lambda.

## Interface

| Type | Purpose |
|------|---------|
| `Emitter` | LIR → `ClosureCompiled`, writing each nested lambda's payload into the `CodeArena` it was built over ([mod.rs](mod.rs)). `emit_module_with_lambdas` also answers the header over every closure's payload, for the WASM backend's dual compile |
| `ClosureCompiled` | What emitting one function produces: the bytecode, and the yield points, call sites and charge sites emission records |
| `YieldPointInfo` | What emission records at a yield point: resume IP, live registers, local count |
| `CallSiteInfo` | What emission records at a call or an instruction that may raise: resume IP, live registers, local count, and whether a resume delivers a value |
| `ChargeSiteInfo` | What emission records at a fuel charge site: the charging instruction's IP, live registers and local count, for a fuel pause in compiled code |

## One function

```text
Emitter::emit, reading one function through a LirView
    ├─► find the registers live into each block (live.rs)
    ├─► simulate the operand stack to place each register (stack.rs)
    ├─► emit instruction bytes, then patch jump offsets
    ├─► build the LocationMap from the node spans
    ├─► collect YieldPointInfo at each Emit terminator
    ├─► collect CallSiteInfo at each call and each instruction that may
    │   raise, in a function that may signal
    └─► collect ChargeSiteInfo at each instruction that charges fuel, in a
        function that may signal
    │
    ▼
ClosureCompiled { bytecode, yield_points, call_sites, charge_sites }
```

The emitter emits blocks in the order the lowerer appended them, so it meets
each predecessor of a merge block before the merge. `emit_module` emits the
closures first, from the highest closure id down. A child's id is higher than
its parent's, so a parent's `MakeClosure` finds its child already emitted.

## Rules

1. **A value computed before an emit survives into the resume block.** The
   emitter carries the stack simulation across the emit through
   `yield_stack_state`, so the `1` in `(+ 1 (emit :yield 2) 3)` is still on
   the stack when `LoadResumeValue` runs.

2. **Yield, call-site and charge-site metadata come from emission.** The
   emitter records a `YieldPointInfo` at each `Terminator::Emit`, and a
   `ChargeSiteInfo` at each instruction that charges fuel. It records a
   `CallSiteInfo` at each call, each `TailCall`, and each instruction that may
   raise (`Op::may_raise`, [raise.rs](../code/raise.rs)).
   `PayloadParts::lambda` hands all three to the code payload's LIR body beside
   the frozen records, and the JIT reads the payload's.

3. **Call sites and charge sites are recorded only where the function may
   signal.** `Emitter.current_func_may_suspend`, set from
   `signal.may_suspend()`, gates the recording. That method is true for any
   signal bit, an error included, so only a silent function has neither. A
   silent function never parks: its charges spend fuel without pausing
   ([jit.md](../../../docs/impl/jit.md)).

4. **A block's first emitted predecessor fixes its operand depth.** Every
   other edge into that block must arrive at the same depth, as the last
   section says.

## Yield, call-site and charge-site metadata

`YieldPointInfo`:
- `resume_ip` — the bytecode offset to resume at, after the emit opcode.
- `stack_regs` — the registers on the operand stack at the emit, bottom to top.
- `num_locals` — the local slots below them.

The JIT spills the locals, then these registers, and calls the emit runtime
helper.

`CallSiteInfo`:
- `resume_ip` — the bytecode offset after the instruction, where the
  interpreter resumes when a signal leaves the frame there.
- `stack_regs` — the registers on the operand stack after the instruction's
  operands are popped and before its result is pushed.
- `num_locals` — the local slots below them.
- `push_resume_value` — whether a resume delivers into the instruction's
  result. An instruction with no result (`InstrRef::raise_delivers`) takes
  none.

That is the interpreter's stack when a signal leaves the frame at the site, a
yield or an error. The JIT builds the frame's `SuspendedFrame` from it.

A `TailCall` records a call site too. Its `resume_ip` is where the block after
the tail call starts, the block a native callee falls through to when it
completes. A compiled frame whose tail callee suspends parks there, so the
resume runs that block ([park.md](../../../docs/impl/region/park.md)).
`TailCallArrayMut` records none.

`ChargeSiteInfo`:
- `ip` — the bytecode offset where the charging instruction starts. A fuel
  pause parks there, and the interpreter runs the instruction again on resume.
- `stack_regs` — the registers on the operand stack at that offset, bottom to
  top. For a call that is the stack with its arguments and callee still on
  top; for a backward jump it is the stack the edge trimmed to.
- `num_locals` — the local slots below them.

The charging instructions are the interpreter's: `Call`, `CallChecked`,
`TailCall`, `TailCallChecked`, `CallArrayMut`, `TailCallArrayMut`, and a
`Jump` whose target block was emitted before it. Each call instruction records
one site, and so does each `Terminator` that emits a backward `Jump`, in
emission order.

## Merge operand depth

The VM addresses local `n` as `frame_base + n` on the operand stack, so the
entry block reserves `num_locals` positions and operands stack above them
(`Emitter::emit_block`). A path that pops one operand too many falls through
that floor and destroys a live local. The damage shows up much later, as a
`LoadLocal` of a high slot indexing past the end of the stack.

The emitter simulates the operand stack per block. A block starts from the
simulation an edge recorded for it in `yield_stack_state`, because the
simulation cannot reconcile two different incoming shapes. A `Jump` keeps the
first record for its target, and a `Branch` writes its own. That makes one rule
mandatory:

> **The first predecessor emitted fixes the merge block's operand depth, and
> every later edge into that block must leave exactly that depth.**

Every edge trims to meet the rule: a `Jump`, and each of the two edges of a
`Branch`. A forward edge pops its trailing dead cells with
`pop_trailing_dead_to`. A dead cell is an orphan whose register is not live
into the target, so no path from the target reads it. Popping a dead cell is
free.

An orphan is a cell the simulation's register map does not name: the residue
`ensure_on_top` leaves when it copies a value up with `DupN` and the copy is
then consumed. An orphan is not always dead. It can be the only cell that
holds a value a later block reads, such as the limit of a loop. So the emitter
finds the registers live into each block (`live_in`), and the trim pops an
orphan only when the target does not read it.

The trim stops at a cell the map names, read or not, because the simulation can
name a cell the real stack does not hold. After a `TailCall` it keeps the
callee and the arguments, where a native callee leaves its one result.

A `Branch` trims each edge on that edge's own path. The fall-through edge pops
after `JumpIfFalse`. When the taken edge has a cell to pop, `JumpIfFalse` lands
on a pad that pops it and jumps to the target. A branch that left its dead
cells in place would split the paths. In an `and`, the branch edge carries a
dead cell straight to the merge, and the jumps out of a nested `if` pop that
cell on the other path. The merge's successors inherited the branch's
simulation, so they pop it a second time on the path that already did.

The forward trim is **bounded by the target's already-fixed depth**, because
popping past it splits the paths the same way. One edge into the merge leaves
the dead cell, a later edge removes it, and the merge's successors pop it
again. Two pops, one value, and the second one lands in the reserved local
region. So a forward edge trims only down to the target's recorded depth
(`edge_depth`), rather than to the first live value.

### A back edge restores the depth its target was fixed at

The dead-cell trim cannot meet the rule at a back edge. It stops at the first
live cell, so a dead cell the block left *under* a live one survives the jump.
Straight-line code absorbs that residue; a loop accumulates it. The header runs
again a cell or two deeper each time, and the activation's operand stack grows
for as long as the loop does. No region gauge sees that growth, because the
cells are a `Fiber`'s own `SmallVec` rather than heap objects.

A back edge is the one edge that may trim to the target's depth outright, so
it uses `pop_to`, whether a `Jump` or a `Branch` takes it. The target was
emitted before this edge existed, and its simulation names only cells below
that depth, so it reads nothing this block pushed. Every surplus cell goes,
dead and live alike, and the header meets the same stack shape on every pass.

A forward merge keeps the dead-cell trim, because the same argument does not
hold for it. Its target is still ahead of the cursor, and a later edge's own
result may sit above a dead cell, so trimming to the depth would drop the
result and keep the dead cell.
