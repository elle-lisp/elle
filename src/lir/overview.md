# LIR

<!-- audited: 2026-11-27 -->

The LIR module's types, how a reader walks a frozen function, and the rules every block and register keeps.

[The LIR design](../../docs/impl/lir.md) owns the two forms of a function, the
records a frozen function is made of, the operand proof, heap literals and
`LoadSelf`. [The lowerer](lower/overview.md) owns how HIR becomes LIR, and
[the emitter](emit/overview.md) owns how LIR becomes bytecode.
[The data form](../../docs/spec/form.md) is how Elle code reads a function,
through `compile/lir`.

## Interface

The working form, which the lowerer builds ([build/](build/mod.rs)):

| Type | Purpose |
|------|---------|
| `LirBuilder` | Builds one unit's functions in a working region it frees when dropped: a stack of functions under construction, each block's nodes in a `RegionVec`, the splices the relocation makes, and `finish_function`, which freezes a function into a `LirOwned` |
| `LirHead` | The header of the function under construction: name, arity, signal, slot and capture counts, the binding each slot holds, capture masks, docstring, origin, region table, merge set, release tables, the body fragment a graft reads, and the record of each capture `fn/captures` reads |
| `RegionVec` | A slice that grows in the working region: push, truncate, and insert at an index |
| `Lowerer` | HIR → LIR, answering a `FrozenModule` ([the lowerer](lower/overview.md)) |

An instruction, in both forms:

| Type | Purpose |
|------|---------|
| `InstrRef` | One operation: built by the lowerer, encoded by `LirBuilder::emit`, decoded by every reader ([code/instr.rs](code/instr.rs)) |
| `ConstRef` | An immediate constant: nil, the empty list, a bool, a number, a symbol or a keyword |
| `TemplateBytes`, `Slots`, `ConstList`, `Offers` | The borrowed operands an `InstrRef` carries: a `MaterializeConst`'s encoded template, a tail call's stash slots, a `StructRest`'s keys, a call's moves or late mints |
| `Offer` | One entry of `Offers`: an argument position below `Offer::LIMIT`, and the slot the frame's release loads ([handoff.md](../../docs/impl/region/handoff.md)) |
| `OfferBuf` | The offers the lowerer gathers for one call, lent to its instruction as `Offers` |
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
| `LirCode` | The records, tables and header in `Vec`s: plain data, and `Send` |
| `LirOwned` | A `LirCode` plus the `Value`s its `ValueConst` instructions load |
| `LirBody` | The same records in region pages, as a code payload's `lir` field ([code/body.rs](code/body.rs)) |
| `LirView` | The read API over either home: blocks, instructions, terminators, header, tables, sites |
| `SiteRef` | A yield point, a call site or a fuel charge site: the IP the interpreter resumes at, live registers, local count, and whether a resume delivers a value |
| `SlotRef` | One local slot an instruction names, as a read or a store ([code/slots.rs](code/slots.rs)) |
| `SlotLiveness`, `LiveSlots` | The local slots live into each block and around each node, and one such set ([code/live.rs](code/live.rs)) |
| `ChildIndex` | The child-table entry a view's `child_of` names for a `MakeClosure` ([jit-closure.md](../../docs/impl/jit-closure.md)) |
| `form::instr`, `form::terminator` | An instruction or a terminator in the data form: its kind, and each field of its variant by name ([code/form.rs](code/form.rs)) |

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
it uses, without decoding the rest of the instruction. Run
`elle --dump=lir FILE` to print the LIR of a program.

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

## Local slot liveness

A local slot is live at a point when some path from that point reads the slot
before a store replaces it. `SlotLiveness::of` answers that for a frozen
function: the slots live into each block, and the slots live before and after
each node of a block. A JIT side exit spills the slots live where the
interpreter resumes, and no others
([jit-closure.md](../../docs/impl/jit-closure.md)).

`InstrRef::slot_refs` names each local slot an instruction reads or stores.
Its match is exhaustive, so a new variant cannot compile until it says which
slots it names.

| Instruction | Reads | Stores |
|-------------|-------|--------|
| `LoadLocal` | its slot | |
| `StoreLocal` | | its slot |
| `LoadCapture`, `LoadCaptureRaw`, `StoreCapture` | slot `index - num_captures`, where the index lies past the captures and the parameters | |
| `DecrefPostponed` | `slot` and `route` | |
| `Call`, `SuspendingCall` | the slot of each move | |
| `TailCall` | each borrowed-argument slot, and the slot of each late mint | |

A store replaces a slot only where no cell holds the local
(`capture_locals_mask`). A store to a celled local counts as a read. No
terminator names a slot. A `Jump` flows into its label, a `Branch` into both
of its labels, and an `Emit` into its resume block. A `Return` and an
`Unreachable` flow nowhere, and a tail call ends no block. The analysis
counts a slot live wherever it cannot prove the slot dead, so it can overstate
what is live and never understates it.

## Invariants

1. **Each register is assigned exactly once.** SSA form. A register used
   before its definition means lowering is broken.

2. **Every block ends with a terminator.** `Return`, `Jump`, `Branch`, `Emit`
   or `Unreachable`. No fall-through. A tail call is an instruction
   (`TailCall`, `TailCallArrayMut`), not a terminator.

3. **Emit is a block terminator, not an instruction.**
   `Terminator::Emit { signal, value, resume_label }` ends the block, and the
   resume block opens with `LoadResumeValue`, which takes the value passed to
   `fiber/resume`.

4. **Blocks keep the order the lowerer appended them in.** A merge block is
   appended after every block that jumps to it, and freezing keeps the order,
   so the emitter meets each predecessor first. Labels are allocated in
   creation order, so sorting the blocks by label would break that.

## Building LIR in tests

`testkit::LirFixture` ([testkit.rs](testkit.rs), `#[cfg(test)]`) assembles a
frozen function instruction by instruction, for the unit tests of every reader
of LIR. Those readers are the emitter, the JIT, the WASM backend, the MLIR and
SPIR-V tiers, and the cross-thread send path. It mirrors `hir::testkit`
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

Each method's doc comment states its rule, and the test for each rule sits at
the bottom of [testkit.rs](testkit.rs). A frozen function's fields are
read-only, so a test sets a header field through the fixture.

`InstrRef::call`, `InstrRef::suspending_call` and `InstrRef::tail_call` build a
call that offers nothing ([code/call.rs](code/call.rs)). A test whose subject is
not the handoff builds its calls through them.

## Size

[code/instr.rs](code/instr.rs) is far past the size cap, so it carries no
audit stamp and is exempt by name from the audit queue and the size ledger
([docs/impl/audit.md](../../docs/impl/audit.md)). The file is one enum,
`InstrRef`, with a doc comment per variant, and Rust gives no way to split one.
Bringing it inside the cap means nesting a group of variants into a sub-enum,
which rewrites every exhaustive match in the crate. That is its own change, not
a rider on whatever touches the file next.

A match over the instruction set splits where an enum cannot:
[emit/instr/ops.rs](emit/instr/ops.rs) hands its tail to
[ops/intrinsics.rs](emit/instr/ops/intrinsics.rs), and every other file here
takes the ordinary budget. The encoder lives in [build/](build/mod.rs) and
the decoder in [code/decode.rs](code/decode.rs), each apart from the enum.
