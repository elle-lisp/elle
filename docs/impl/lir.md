# LIR — Low-level IR

<!-- audited: 2026-10-06 -->

LIR is an SSA-form intermediate representation with virtual registers,
basic blocks, and explicit control flow.

## Two forms of one function

The lowerer builds a function in a **working form** it can grow and splice.
Freezing copies the finished function into a **frozen form**: fixed-size plain
records, every operand list in one pool, read through a borrowed view. Every
reader of LIR reads the frozen form — the emitter, the JIT, the WASM, MLIR and
SPIR-V backends, `send`, the dumps and the introspection primitives. Only the
lowerer and its own analyses read the working form.

Both forms hold the same 48-byte node. They differ in where the nodes live and
whether they can still move. The lowerer pushes a node at a time and splices
into blocks it has already finished, so the working form keeps each block's
nodes in a slice that grows. A reader walks every node once, and a code object
keeps its LIR for as long as the JIT may promote it, so the frozen form keeps
every block's nodes in one exact-size table.

One type names an instruction in both forms. **`InstrRef<'a>`** is what the
lowerer builds and what a reader decodes. It holds borrowed slices where an
instruction carries a list, so building one allocates nothing.

### The working form

A **`LirBuilder`** ([src/lir/build/](../../src/lir/build/mod.rs)) builds the
functions of one compilation unit in a **working region** of its own on the
instance's heap. The region lives for one call to `Lowerer::lower`: lowering
mints it, and the builder frees it when `lower` returns, whether lowering
succeeded or failed. Nothing the lowerer returns points into it.

- **`RegionVec<T>`** — a slice that grows in the working region. A region
  bumps its data down from the top of a page, so nothing can be appended to an
  extent already written. A full `RegionVec` claims an extent twice the size
  and copies into it, and the old extent is dead until the working region is
  freed. [measurements.md](image/measurements.md) item 8 measured the shape:
  it holds parity with a `Vec` per block, and the bytes that growth leaves
  behind last only as long as the compile.
- **The function under construction** — a `RegionVec` of nodes per block,
  and the function's pool, constants, template bytes, values and file table,
  each a `RegionVec` too. Its header is **`LirHead`**: the name, arity,
  signal, slot and capture counts, capture masks, vararg kind, rest-list
  layout, region table, merge set and release tables, as plain fields the
  lowerer writes.
- **`begin_function`** stacks a function. A lambda's body is lowered while
  its parent's open block is half built, so the parent's state waits on the
  stack, untouched, and resumes when the lambda finishes.
- **`emit`** encodes an `InstrRef` into a node at the end of the open block:
  its registers inline, any longer register list and the variant's other
  scalars appended to the function's pool, its constants to the constant
  table. **`terminate`** sets the open block's exit, and a block never
  terminated exits `Unreachable`. **`finish_block`** closes it.
- **A splice moves nodes.** The relocation ([lower/AGENTS.md](../../src/lir/lower/AGENTS.md)
  § "Tail-call ownership") emits a release run at the end of the open block,
  takes the run back out, and inserts it ahead of a tail call: in the open
  block, or in a block already finished. Only the 48-byte nodes move. Each
  node's pool words stay where `emit` wrote them, and a node names its pool
  run by offset, so the move cannot disturb an operand.
- **The lowerer reads its own nodes back** as `InstrRef`, decoded against the
  function's tables exactly as a reader decodes a frozen node. The relocation's
  questions about a run — what slot it loads, whether it stamps the slot nil
  — are asked that way.
- **`finish_function`** freezes the function. It copies every block's nodes
  into one node table and every table into an exact-size `Vec`, moves the
  header across, and answers a `LirOwned`. A finished function points into no
  region, so it outlives the working region the builder frees.

`Lowerer::lower` answers a **`FrozenModule`**: the entry function and every
closure its `MakeClosure` instructions name by `ClosureId`, each a `LirOwned`.

`PushParamFrame` carries its (parameter, value) pairs as one flat register
list, parameter first.

### Instructions and constants

- **`Reg`** — virtual register (SSA — each assigned exactly once)
- **`Label`** — block label for control flow
- **`InstrRef`** — an operation (load a constant, add, call, and the rest).
  Its variants carry the per-instruction documentation
  ([code/instr.rs](../../src/lir/code/instr.rs)).
- **`Terminator`** — block-ending instruction (return, jump, branch, emit,
  unreachable). Tail calls are `InstrRef` variants
  (`TailCall`/`TailCallArrayMut`), not terminators.
- **`ConstRef`** — a compile-time **immediate** constant: nil, the empty
  list, a bool, an int, a float, a symbol or a keyword — all tag+payload, no
  heap. `Const`/`ValueConst` are pure pool loads with no `region` field. No
  constant is a string: a string literal is a `MaterializeConst`, in a
  pattern as everywhere else.
- **`MaterializeConst`** — the allocation that builds a *heap* literal
  (a string, or quoted compound data: list / array / nested structure) from
  a recursive immutable `ConstTemplate` ([template.rs](../../src/value/template.rs)) into **its
  own** solver-assigned region. It carries a mandatory `region: StaticRegion`
  and is an ordinary allocation site (see *Heap literals are allocations*
  below). The whole aggregate shares the one region (built bottom-up, so every
  internal reference is a self-edge taking no cross-region RC). The
  instruction carries its template as the bytes `ConstTemplate::encode`
  writes.

### The frozen form

Freezing writes these records ([src/lir/code/](../../src/lir/code/mod.rs)):

- **`Op`** — one opcode byte per `InstrRef` variant, matched exhaustively, so
  a new variant cannot be encoded until it has an opcode.
- **`Node`** — one instruction in 48 bytes of plain data with no implicit
  padding: the span (`start`, `end`, `line`, `col`, and a `file` index into the
  function's own file table), `dst`, `region`, `aux`, the first two register
  uses, an `extra` offset into the pool, the use count, the opcode and a flag
  byte. Every byte is written, so a copy of a node is a copy of its meaning.
- **`BlockRec`** — a label, the block's range of nodes, the terminator's
  fields and span, and an explicit zero pad.
- **`ConstRec`** — a kind byte, an explicit seven-byte pad, and 64 bits.
- **`SiteRec`** — a yield point or a call site: its resume address, its local
  count, and a range of the site-register table.

A function names its own parts by index everywhere. A register list longer
than two lives in the pool, so a node is the same size whatever it carries. A
node names its span's file by an index into the function's file table, never by
a process-wide `FileId`.

Each constant has one home:

| Operand | Where it lives |
|---------|----------------|
| a `ConstRef` immediate | a `ConstRec` |
| a `ValueConst` value | the function's `values` table |
| a `MaterializeConst` template | the `data` bytes, as `ConstTemplate::encode` wrote them |
| a capture cell's name, a signal bound's mask, an `Emit`'s signal | a `ConstRec` of raw bits |

Every `ValueConst` value is also in the bytecode constant pool, which is what
keeps it alive. A debug build checks this after emission. The template encoding
names symbols and files by their spelling, so it means the same thing in every
process.

A frozen function has two homes, and both hold the same records:

- **`LirCode`** holds the records, the tables and the function's header in
  `Vec`s. It is plain data: `Send`, and serializable with serde.
  **`LirOwned`** is a `LirCode` plus its `values: Vec<Value>`.
  `finish_function` produces one per function, and a `JitTask` holds one.
- **`LirBody`** holds them in region pages, as the `lir` field of a code
  payload ([region/template.md](region/template.md)). Every field is a
  `RegionSlice` or a scalar, so the body is sealed data and an image carries
  it with the rest of the payload. Its file table holds spellings rather than
  `FileId`s, so a body holds no process-local number and needs no file stream.

A body carries only what LIR alone knows: the closure id, the entry label, the
register count, the capture and local-parameter counts, and the sites. The
header fields the two share are the payload's, and a view over a payload reads
every one of them off that one record: the name, the docstring, the origin, the
arity, the signal, the local and parameter counts, the capture masks, the
vararg kind, the rest-list layout, the region table, the merge set and the two
release tables. Freezing records the merge set and the release tables
ascending, so a `LirCode`'s tables and a payload's agree on order as well as
content.

**`LirView<'a>`** is the one read API, over either home. It borrows the slices
of a frozen function and answers its blocks, its instructions, its terminators,
its header and its tables. `LirView::to_owned` copies everything the view reads
into a `LirOwned`, which is how a function leaves a payload for another thread.
A node decodes into the `InstrRef` it was encoded from, field for field: a
register list as `&'a [Reg]`, a template as `TemplateBytes<'a>`, a tail call's
stash slots as `Slots<'a>`, and a `StructRest`'s keys as `ConstList<'a>`.

Decoding is safe Rust over slices. A corrupt index panics where it is read,
and never reads outside the function's own records.

## From HIR to LIR

The lowerer ([src/lir/lower/](../../src/lir/lower/AGENTS.md)) transforms HIR
trees into LIR:

1. **Flatten** — nested expressions → linear instruction sequences
2. **Register allocation** — each intermediate value gets a virtual
   register
3. **Block construction** — control flow (if, loops, match) creates
   basic blocks connected by terminators
4. **Region assignment** — every allocation is routed to a region
   (see [regions](../regions.md)); the lowerer emits each region's release
   after the HirId the solver names as its `decref_point`, and `IncrefRegion`
   at cross-region edges

## From LIR to its readers

```text
Lowerer ──► LirBuilder: the working form, in a working region
              │
              ▼  finish_function freezes each; lower frees the region
          FrozenModule: one LirOwned per function
              │
              ▼  Emitter reads each through a LirView
          ClosureCompiled = (Bytecode, yield points, call sites)
              │
              ▼  PayloadParts::lambda, written into the unit's code region
          code payload: a LirBody, read through ClosureTemplate::lir()
              │
              ├─► JIT worker: a JitTask owns the view's to_owned copy
              ├─► WASM, MLIR, SPIR-V, introspection: the view itself
              ├─► send: the records, plus the values through the value walk
              └─► image: the body, copied with the rest of the payload
```

Freezing runs once per compiled function, as the lowerer finishes it, and
always before emission. The yield points
and call sites are the one part only emission can supply, so the emitter
writes them into the payload's body beside the frozen records, at the
`MakeClosure` that builds the lambda ([region/template.md](region/template.md)).
Every reader of a code object reads the body.

## The operand proof

A `%`-intrinsic in call position compiles only when the front end discharges its
operand contract ([intrinsics.md](../intrinsics.md)). `BinOp`, `Compare` and
`UnaryOp` carry that proof across the LIR boundary, so no backend re-derives at
run time what the compiler already decided. `OperandProof::Int`
says every operand of that instruction is an integer on every path reaching it.
`OperandProof::Unproven` claims nothing. The lowerer reads each operand node's
inferred type out of `TypeInfo` — the same map the contract check discharged
against — and marks the instruction `Int` when every one of them is exactly
`int`. Nothing downstream may set the proof: a backend that guessed would be
asserting what only the front end can know.

Build an instruction with `InstrRef::binop`, `compare` or `unary` to claim
nothing, and with `int_binop`, `int_compare` or `int_unary` to carry the proof.

What each backend spends it on:

| Backend | Unproven | Int |
|---------|----------|-----|
| bytecode | `Add` `Sub` `Mul` `Div` | `AddInt` `SubInt` `MulInt` `DivInt` |
| JIT | tag check, then the integer path or a helper call | the integer instruction alone |
| WASM | tag check, then the `i64` path or the `f64` path | the `i64` instruction alone |
| MLIR, SPIR-V | operand types from local inference | the integer operation |

The bytecode set specializes four operations, so a proven `%rem`, `%bit-and` or
comparison still emits the polymorphic opcode. Those handlers already read their
operands as integers and do no better with the proof.

A division keeps its zero test on every tier. The contract does prove the divisor
nonzero wherever both operands are proven integers, but `OperandProof` names the
operand type and says nothing about a value, and a trapping `sdiv` is the wrong
place to spend a reading it does not carry.

An unproven instruction is correct everywhere and only slower, so a lowering that
cannot decide says `Unproven` and each backend does what it did before.

## Heap literals are allocations

A heap literal is an ordinary allocation, **not** a pre-baked `Value`. The
constant pool stores only the literal's immutable *template* — a
recursive `ConstTemplate` (string bytes, or a quoted list, array or nested
structure) as compile-time data, encoded inline in the (reclaimable) bytecode
and held as a `Box<ConstTemplate>` by `JitCode`.
`MaterializeConst` reads that template and allocates a fresh heap value every time
it runs into **its own** region — the solver gives each literal its own
`region: StaticRegion` (`alloc_here`) and `decref_point` exactly like `List`/
`MakeArrayMut`/`MakeClosure`, resolved per activation to a fresh physical region
and allocated into that explicit region (`alloc_in_region`). A quoted aggregate's whole
structure shares that one region. Normal escape RC keeps any escaped copy alive
past its `decref_point`. Only immediates (numbers, bools, nil, interned
keyword/symbol) remain as plain pool constants loaded by `Const`/`ValueConst`.

A template carries symbols **by name**, not by interned id. The id would in fact
survive a `sys/spawn` boundary now that it is the name's hash
([symbol.md](symbol.md)), but the name is what makes the template readable and
self-describing; `materialize` interns it, which records the spelling for
display and returns that same id.

[region/model.md](region/model.md) says why a code-object-lifetime
"constant-pool region" is forbidden.

## Self-reference: `LoadSelf`

A closure that references itself in **value** position — passed to a
higher-order call, returned, or stored, then invoked later — lowers that
reference to `LoadSelf { dst }`. The op takes no operand and pushes the
**currently-executing closure**: the runtime holds the executing closure in a
per-activation register (`current_closure`, [fiber.rs](../../src/value/fiber.rs)), and the JIT
receives that same closure value as a compiled-body parameter, so `LoadSelf`
reads it directly rather than naming a capture slot. The value it yields is the
closure itself, so an invocation of that value recurses correctly
([selfrec.rs](../../src/runtime/tests/selfrec.rs),
[recur-as-value.lisp](../../tests/lang/recur-as-value.lisp),
[recur-after-tail-call.lisp](../../tests/lang/recur-after-tail-call.lisp)).

A self-reference in **call** position (`(loop args)`) lowers its callee to
`LoadSelf` too, so the call re-enters the same code and environment with new
arguments.

## Files

| Path | Contents |
|------|----------|
| [src/lir/types/](../../src/lir/types/mod.rs) | `Reg`, `Label`, `ClosureId`, `Terminator`, the operators, and the site records emission produces |
| [src/lir/build/](../../src/lir/build/mod.rs) | The working form: `LirBuilder`, `LirHead`, `RegionVec`, and the encoding of an `InstrRef` into a node |
| [src/lir/code/](../../src/lir/code/mod.rs) | The frozen form: the records, `LirCode`, `LirOwned`, `FrozenModule`, `LirBody`, `LirView`, and `InstrRef` with its operands |
| [src/lir/display.rs](../../src/lir/display.rs) | Debug printing of LIR |
| [src/lir/lower/](../../src/lir/lower/AGENTS.md) | Lowering from HIR |
| [src/lir/emit/](../../src/lir/emit/mod.rs) | Bytecode emission from LIR |

[src/lir/AGENTS.md](../../src/lir/AGENTS.md) describes the types and the
emitter.

---

## See also

- [impl/hir.md](hir.md) — HIR analysis before lowering
- [impl/bytecode.md](bytecode.md) — bytecode emitted from LIR
- [impl/jit.md](jit.md) — JIT translates LIR directly
