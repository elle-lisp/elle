# The facts of the region IR

<!-- audited: 2026-11-24 -->

The facts each instruction of the region IR gives, and the instructions the verifier refuses.

[The region IR](ir.md) says what the IR holds, and [the data form](form.md)
how Elle code reads it. [Datalog](datalog.md) and the documents it names
define each fact below. They write the facts of the kernel forms by hand, and
this document derives the same facts from a function of the IR.
[The points](points.md) gives each step's point, its `next`, and what a jump
hands to a block's parameters.

## Values and callees

A value is named by the register that holds it. A step that takes a value out
of a container, where no register receives the value, names it `[:out p]`, as
[datalog](datalog.md) says. A step that reads the elements of a container as
one value names them `[:elem p]`.

A call names its callee by what wrote the callee's register:

| The register's writer | The callee |
|-----------------------|------------|
| a ValueConst of a native | the native's name in `compile/primitives` |
| a ValueConst of a closure | `[:constant k]`, the closure constant `k` of the unit, numbered in order of first sight and compared with `identical?` |
| a MakeClosure of the closure id `c` | `[:closure c]` |
| a LoadSelf in the function whose closure id is `c` | `[:closure c]` |
| anything else | the register |

## The rules

Every register an instruction or a terminator reads gives `use(v, p)` for the
value it holds, a callee and a branch's condition included. A Release gives
no use, and an argument a jump hands to a block parameter gives `pass` and no
use. Every kind gives the facts of [the points](points.md) beside those below.
A refused kind gives `next` and `use` alone, and the function records
the refusal. In each row, `d` is the register the instruction writes, `s` the
container or the scrutinee it reads, `c` a cell, and `i` an index.

| Kind | Facts |
|------|-------|
| Param | `param(d, i)`, and no other fact |
| Release | `release(v, p)`, and no other fact |
| Const | `immediate(d)` |
| ValueConst | `immediate(d)` for an immediate or a native, and `free(d)` for any other value |
| MaterializeConst | `alloc(d, p)` and `immutable(d)` |
| LoadCaptureRaw | `free(d)` and `captured(d)`, at an index below the capture count |
| LoadSelf | `free(d)` |
| MakeClosure | `alloc(d, p)`, and `capture(x, d, p)` for each capture `x` |
| List | `alloc(d, p)`, `immutable(d)`, and `operand(x, d, p)` for its head and its tail |
| MakeArrayMut | `alloc(d, p)`, and `operand(x, d, p)` for each element |
| StructRest | `alloc(d, p)`, `immutable(d)`, `read([:elem p], s, p)` and `operand([:elem p], d, p)` |
| MakeCaptureCell | `alloc(d, p)` and `operand(x, d, p)` for its value |
| LoadCaptureCell | `read(d, c, p)` |
| StoreCaptureCell | `store(x, c, p)` and `remove([:out p], c, p)` |
| First, Rest, FirstOrNil, RestOrNil, FirstDestructure, RestDestructure, ArrayMutRefDestructure, ArrayMutRefOrNil, StructGetOrNil, StructGetDestructure | `read(d, s, p)`, `s` the container |
| Pop | `remove(d, s, p)` |
| ArrayMutPush | the facts of a call to the native `%push-array-mut` with the array and the value |
| BinOp, UnaryOp, Convert, Compare, Identical, Length, Has, TypeOf, ArrayMutLen, the type checks | `immediate(d)` |
| CheckSignalBound | `use` alone |
| Call, SuspendingCall | `call(d, f, p)` and `arg(y, i, p)` for each argument `y` at position `i`, and a native callee's spec facts |
| TailCall | the facts of a Call, and `return(d, p)` |
| MatchFail | `cross(s, p)` for the scrutinee and `alloc(d, p)` |
| Eval | `alloc(d, p)`, where its environment register holds an immediate |
| Return | `return(v, p)` |
| Emit | `cross(v, p)` |
| Jump, Branch, Unreachable | no fact but those of [the points](points.md) |

The type checks are IsNil, IsPair, IsArray, IsArrayMut, IsStruct, IsStructMut,
IsSet, IsSetMut, IsEmpty, IsBool, IsInt, IsFloat, IsString, IsKeyword,
IsSymbolCheck, IsBytes, IsBox, IsClosure and IsFiber. Each answers an
immediate and allocates nothing, so its value has no owner fact.

A native callee's spec facts are [the signature](signature.md) of its
`:spec`, read at the call. A resumed parameter gives `resume(y, p)`, a
crossing parameter `cross(y, p)`, and a payload result `payload(d, y, p)`. A
fresh result gives `alloc(d, p)`, and a fresh immutable result also gives
`immutable(d)` and `operand(y, d, p)` for each argument at a returned
parameter. A parameter stored into a place gives `store(y, c, p)`, `c` the
argument `j` for `[:param j]` or `d` for `:result`. Every other mode and kind
is the callee's signature, which [call](call.md) reads.

A cell is a container of one value, so an overwrite takes the old value out,
as `put` does in [datalog](datalog.md).

## What the verifier refuses

A kind whose result depends on a run-time tag has no rule, and neither has a
kind whose facts the vocabulary cannot state yet. The verifier refuses each
one, beside the kinds [the region IR](ir.md) refuses:

| Kind | Why |
|------|-----|
| LoadCapture, StoreCapture | each checks at run time whether the entry is a cell |
| LoadCaptureRaw at an index past the capture count | it reads a local slot |
| Get | it dispatches on the container's tag |
| Freeze, Thaw | each answers its argument itself where it already has the target mutability, and a copy elsewhere |
| ArrayMutSliceFrom | its result takes the mutability of its input |
| LoadResumeValue | an Emit's resume block takes the resume value as its parameter |
| CallArrayMut, TailCallArrayMut, ArrayMutExtend | a spliced call passes arguments at positions the IR does not hold |
| PushParamFrame, PopParamFrame | no rule says what a parameter frame holds |
| Eval whose environment register holds a heap value | [the ownership rules](ownership.md) do not decide it |
| Call to a native with no `:spec` | [signatures](signature.md) refuse it |

## Not written yet

- The facts of the error exit of an instruction that raises, such as
  FirstDestructure on a value that is not a pair.
- The Datalog rules that read `release`, which the verifier runs.
