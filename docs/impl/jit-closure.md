# How compiled code builds a closure

<!-- audited: 2026-11-27 -->

Compiled code builds a closure with the interpreter's own function, and holds the child's header as a constant.

A `MakeClosure` builds a closure over a nested lambda's code object and its
captures. The emitter wrote that code object when it emitted the parent
([template.md](region/template.md)), so neither tier writes a payload at run
time. Each builds three things in the region the instruction names: a fresh
header over the child's payload, the environment slice, and the instance.

## The call

The translator stores the captures in a stack slot, resolves the
instruction's region slot as every allocating instruction does, and calls
`elle_jit_make_closure`. A merged slot resolves through the mint-or-reuse
helper, as it does in the interpreter ([merging.md](region/merging.md)). The
helper calls `materialize_closure_in_region`, the function the interpreter's
`MakeClosure` handler calls, so both tiers build the same objects in the same
region.

`MakeClosure` is not in `Op::may_raise`, as `List` and `MakeArrayMut` are not.
The emitter records no park site for it, and the translator emits no signal
check after it.

## Which child header

The interpreter's operand is an index into the parent's child table. The
emitter pushes one entry for each `MakeClosure` it emits, walking the blocks
and their nodes in order, so the operand is the instruction's ordinal in that
walk. The emitter writes one code object per lambda and keeps its header by
`ClosureId`, so every entry for one id is the same header `Value`.

`LirView::child_of(id)` answers the ordinal of the first `MakeClosure` that
names `id`, and the translator reads the header at that index. It keeps no
running count, so a block that the translator stops early leaves no count to
repair.

## The header is a constant of the code

The translator bakes the header's two words into the call, as it bakes the
value of a `ValueConst`. An activation can outlive its closure's heap value
([vm.md](vm.md)), so the helper does not reach the child table through the
executing closure. The header lives in the parent's code region, and the cache
entry's `CodePin` holds that region while the code is installed
([jit.md](jit.md)).

The compile takes the parent's child table beside its LIR. The worker's task
carries a copy of the table, as it carries a copy of the LIR, and the worker
reads each header as two words it never follows. The table crosses that
boundary as a type of its own, because it and the `ValueConst` values are both
slices of `Value`. A `MakeClosure` whose index the table does not hold is
invalid LIR.

## The frame a side exit rebuilds

A compiled frame that parks becomes an interpreter frame, so it must be the
frame the interpreter would hold. The interpreter reads a local that a nested
closure captures through a cell in its environment. `populate_env` builds the
environment as the captures, then the parameters, then one slot for each local
past the parameters: a fresh cell where `capture_locals_mask` sets the local's
bit, nil otherwise. The celled local's stack slot holds nil, because nothing
stores to it.

The compiled prologue holds each celled local's cell in that local's variable
(`init_locally_defined_vars`). A side exit stores the parameters, then each
local live where the interpreter resumes, then the operands. Local `l` keeps
its place, `spill[num_params + l]`, a cell included, whether or not the exit
stores it. `rebuild_frame` reads nil for a local the site does not keep, and
moves each cell to the environment:

```text
env   = captures ++ params ++ [ env slot of local l, for num_params <= l < num_locals ]
stack = [ stack slot of local l, for 0 <= l < num_locals ] ++ operands

kept(l)    = l is live where the site resumes
celled(l)  = l >= num_params and bit (l - num_params) of capture_locals_mask
value(l)   = kept(l) ? spill[num_params + l] : nil
env slot   = celled(l) ? value(l) : nil
stack slot = celled(l) ? nil : value(l)
```

A celled local `l` therefore sits at environment index `captures + l`, where
the interpreter's `LoadUpvalue` reads it.

[Local slot liveness](../../src/lir/overview.md) answers which locals a site
keeps:

| Site | The interpreter resumes at | The locals the exit stores |
|------|----------------------------|----------------------------|
| A call, or an instruction that may raise | the node after it | those live after it |
| A tail call that parks | the node after it | those live after it |
| An `Emit` | its resume block | those live into that block |
| A fuel charge on a backward jump | the jump | those live into its target |
| A fuel charge on a call | the call | those live before it |

A long function adds locals and sites together. An exit that stored every
local would make the spill code grow with the function's length squared, and
the register allocator's work over the values the spills keep alive with it.

Nothing reads a local the site does not keep. The resumed body reads a slot
only through an instruction that names it. The walk over an abandoned frame
reads a slot only to run a release still owed, and that release's own
`LoadLocal` keeps the slot live ([unwind.md](region/unwind.md)). The buffer is
one slot every exit of the function shares, so a local the exit skips holds a
value an earlier exit stored. `rebuild_frame` never reads it.
