# Glean

<!-- audited: 2026-11-22 -->

Glean reads a unit in the data form and writes the base facts the Datalog rules read.

Glean reads a unit as `compile/lir` answers it, in
[the data form](../../docs/spec/form.md), and reads `compile/primitives` for
each native. [Datalog](../../docs/spec/datalog.md) and the documents it names
write the base facts of each fence by hand. Glean derives the same
facts from the LIR of a real program, and the rules derive every other fact.
It is written in Elle, as a module whose function answers `{:glean glean}`:

| File | What it holds |
|------|---------------|
| [glean.lisp](glean.lisp) | `glean`, over each function of a unit |
| [points.lisp](points.lisp) | The points in view order, and each point's `next` |
| [names.lisp](names.lisp) | The name of the value each register holds |
| [kinds.lisp](kinds.lisp) | The fields of each kind that hold a register, and the kinds marked or skipped |
| [rules.lisp](rules.lisp) | The facts of each kind, and a function's answer |
| [calls.lisp](calls.lisp) | The facts of a call, and a native's `:spec` |

## What it answers

Glean answers `{:functions [...] :closures [...]}`. `:functions`
holds one struct per function of the unit, in the unit's order. `:closures`
holds the closure constants it met, closure `k` at index `k`.

| Key | Value |
|-----|-------|
| `:id` | the function's closure id, or nil for the entry function |
| `:name` | the function's `:name` in the data form |
| `:facts` | a struct from each fact's key to its tuples, keyed as the fences key them |
| `:marks` | a struct from each unruled kind or undeclared native the function holds to its count |

Each fact means what the document that defines it says:
[datalog](../../docs/spec/datalog.md), [call](../../docs/spec/call.md),
[return](../../docs/spec/return.md), [crossing](../../docs/spec/crossing.md)
and [payload](../../docs/spec/payload.md). Glean answers the facts
under these keys, each tuple as the fences write it.

| Key | Tuple | Key | Tuple |
|-----|-------|-----|-------|
| `:next` | `[p q]` | `:use` | `[v p]` |
| `:alloc` | `[v p]` | `:call` | `[r f p]` |
| `:read` | `[v c p]` | `:arg` | `[y i p]` |
| `:capture` | `[x k p]` | `:return` | `[v p]` |
| `:pass` | `[y x p]` | `:param` | `[x i]` |
| `:operand` | `[x v p]` | `:free`, `:captured` | `n` |
| `:immutable` | `v` | `:cross` | `[y p]` |
| `:resume` | `[f p]` | `:payload` | `[v f p]` |

No rule below gives `store` or `remove` yet, and a kind that stores or removes
is unruled.

## Points

One point per LIR node: each instruction and each terminator, numbered in view
order; `next` follows instruction order and each terminator's targets. View
order takes the blocks in the order of `:blocks`, and each block's
instructions before its terminator.

## Names

A value is named by a binding's symbol, as the data form spells it, or by one
of the compound names below. A register is written once
([LIR](../../src/lir/overview.md)), so the value a register holds is the value
its one write names.

| Name | What it names |
|------|---------------|
| `[:slot s]` | the value of a slot whose `:slots` entry is nil or `:synthetic` |
| `[:reg r]` | the value of a register no slot receives |
| `[:capture i]` | the free name of the capture `i` whose `:captures` entry is `:synthetic` |
| `[:const p]` | the free name a ValueConst of a heap value at `p` gives |
| `[:fn k]` | the closure constant `k` of `:closures`, as a callee |
| `[:exit p]` | the exit of the Return or the TailCall at `p` |

- **Values, by slot through loads.** A StoreLocal of a value no slot holds
  names it by that slot's binding; a LoadLocal reads the value its slot holds;
  a StoreLocal of a named value into another slot is a pass (a jump to a block
  parameter). A register no slot receives is named `[:reg r]`.
- **First store names.** The first slot in view order that receives a value
  names it; a later StoreLocal into another slot is a pass; a StoreLocal into
  the slot that already holds the value gives no fact.
- **Unnamed slots.** A slot whose `:slots` entry is nil or `:synthetic` names
  its value `[:slot s]`; a compiler-made binding with a name, such as
  `__scrutinee`, keeps that name.
- **Immediates.** A constant immediate (nil, the empty list, a boolean, a
  number, a symbol, a keyword) is no value, so no fact names it, and a register
  that holds one names nothing. A slot that receives no value holds none, so a
  load from it names nothing.
- **Slots, to a fixpoint.** A load names what its slot holds, and a slot
  holds a value once a StoreLocal stores a register that names one. Glean
  repeats its round over the stores until no slot changes, because a loop can
  fill a slot from one that a later store fills. Each parameter's slot holds
  its value from the start.
- **Computed immediates.** The result of an instruction is a value, though it
  holds an immediate at run time: arithmetic, a compare, a type check,
  `Length`, `Has`, `Identical`, `TypeOf` and the rest. Its instruction gives no
  `alloc`, because it allocates nothing, so the value has no owner fact.
- **Environment loads.** LoadCapture and LoadCaptureRaw at an index below the
  capture count read the capture's free name; at any other index they read
  slot (index less the count), as LoadLocal does. A parameter and a local cell
  load this way.
- **Synthetic captures.** A capture whose `:captures` entry is `:synthetic`
  has the free name `[:capture i]`, `i` its environment index.
- **Closure constants.** A closure constant is identified by identity: glean
  keeps a table of the closure constants it meets, compared with
  `identical?`, and names each callee `[:fn k]`, `k` its index in that table.

## Facts by kind

Each kind below has one rule. A kind with no rule is marked, and the section
on marks lists those kinds.

| Kind | Facts |
|------|-------|
| the function | Parameter `i` gives `param(x, i)` with `x` its slot's binding, and each capture gives `free(n)` and `captured(n)` with `n` its binding |
| every kind | Every register an instruction or a terminator reads gives `use(v, p)` for the value it holds, a call's callee, a branch's condition and a marked instruction's operands included. A StoreLocal and the count instructions of the last row are the exceptions: they give no use |
| Const | nothing |
| LoadLocal, StoreLocal, LoadCapture, LoadCaptureRaw | the names above, and no other fact |
| BinOp, UnaryOp, Convert, Compare, the type checks, ArrayMutLen, Length, Has, Identical, TypeOf, CheckSignalBound | `use` alone |
| ValueConst | A ValueConst of a heap value other than a function names a free name `[:const p]`; a ValueConst of a function is the callee name only |
| List, MakeArrayMut | `alloc(d, p)` and `operand(x, d, p)` for each heap operand `x`; List also gives `immutable(d)` |
| MaterializeConst | `alloc(d, p)` and `immutable(d)` alone, because a template holds no operand of the function |
| MakeClosure | `alloc(k, p)` and `capture(x, k, p)` for each heap capture `x` |
| First, Rest, FirstOrNil, RestOrNil, FirstDestructure, RestDestructure, ArrayMutRefDestructure, ArrayMutRefOrNil, StructGetOrNil, StructGetDestructure | `read(d, c, p)` |
| Call, SuspendingCall, TailCall | `call(d, f, p)` and `arg(y, i, p)` for each heap argument `y` at position `i`; `f` is a native's name, a closure constant's `[:fn k]`, or the callee register's value |
| TailCall | also `return(d, p)` and `next(p, [:exit p])`, beside its fall-through `next` |
| Return | `return(v, p)` and `next(p, [:exit p])`, an exit of its own |
| Jump, Branch | `next` to the first point of each target block |
| Emit | `cross(v, p)` and `next` to its resume block; an error emit crosses too |
| LoadResumeValue | `alloc(d, p)` |
| MatchFail | an `:error` emit whose payload is a fresh struct holding the scrutinee `s`: `cross(s, p)`, `alloc(d, p)` for the resume value in `d`, and `next` to the following point |
| Eval | `use(d, p)` for its datum and `alloc(r, p)` for its result; an Eval whose environment register names a value is marked |
| Unreachable | no `next` |
| IncrefRegion, DecrefRegion, IncrefValueRegion, DecrefValueRegion, DecrefCellRegion, DecrefPostponed, AdoptRegion, AdoptCellRegion, AdoptIntoActivation, FreeRegionGroup, AssertRegionMatches | no fact, no use either |

Each Return has an exit of its own, because a shared exit would keep a value
returned on one path live behind every other return. A TailCall returns at its
own point, because a closure callee replaces the frame there; its fall-through
runs only for a native callee. A count instruction belongs to the present
release protocol, which a reader of the present releases reads and glean
does not.

Glean refuses a kind the data form does not name, and a call whose callee
names no function and no value. The analyzer refuses a call on an immediate
constant ([functions](../../docs/functions.md)).

## Marks

A kind with no rule marks its function with the kind. It gives `next` and
`use` as every kind does, and no other fact, and the function still gives the
facts of its ruled instructions. The kinds with no rule are the cell
instructions (MakeCaptureCell, LoadCaptureCell, StoreCaptureCell),
StoreCapture, ArrayMutPush, ArrayMutExtend, ArrayMutSliceFrom,
StructRest, Get, Put, Del, Pop, IntrPush, IntrStringPush, IntrBytesPush,
Freeze, Thaw, CallArrayMut, TailCallArrayMut, PushParamFrame, PopParamFrame
and LoadSelf. Get is among them because it dispatches on the run-time tag.
The marks, counted per kind and per native across the census, rank the open
rules.

## Natives

Glean names a native callee by its name in `compile/primitives`, found by
identity of `:fn`, and reads the native's signature from its `:spec`
([signatures](../../docs/spec/signature.md)). A resumed parameter gives
`resume(y, p)`, a crossing parameter `cross(y, p)`, and a payload result
`payload(d, y, p)`, with `y` the argument at that position. Its other modes and
kinds are the callee's signature, which [call](../../docs/spec/call.md) reads.
A native with no declaration makes its call unruled, and the call marks its
function with the native's name, the string `compile/primitives` gives. The
region declarations cannot carry such a signature, because they spell several
natives with different signatures as one `funnel`.

## Tests and the census

Glean's tests are language tests: files named `glean-*.lisp` at the top of
[tests/lang](../../tests/lang/overview.md), with their shared helpers in a
`glean` directory there. Each feeds a hand-built unit in the data form and
reads no implementation artifact, so its claim holds on every build. Each
compares the answer with facts derived by hand, as
[the region IR](../../docs/spec/ir.md) says a verifier test is built.

The census is not built yet. A make target imports every document of
[the spec](../../docs/spec/AGENTS.md) in one process, as
[the specification](../../docs/spec.md) lays each one out, and takes every
fence whose `:source` is a string. It compiles that source, gleans its facts,
and runs one fixed judge over both fact sets: `released-all` and `refusals`,
each answer as the set of value names, points dropped. A glean symbol counts
as the keyword of the same spelling, and every other name is compared as it
is. Each mismatch is counted, not failed. A fence built from another fence has
no source, and the census skips it.

A census row records the document, the name, whether the source compiled or
the compile error, the marks per kind, and each name in one answer and not the
other. The present lowering hoists constants, inlines compares, adds tail calls
and synthetic slots, and lowers `push` and `put` to a type dispatch with an
`:error` emit, so its facts do not equal the hand facts.
