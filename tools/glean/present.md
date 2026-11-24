# The present releases

<!-- audited: 2026-11-24 -->

Where this implementation's count instructions end a function's hold on each value's region, read at glean's points.

[Glean](overview.md) gives the facts from which the Datalog rules place each
release. The present lowering places its own releases as count instructions,
which glean skips. `present` ([present.lisp](present.lisp)) reads those
instructions from a unit in [the data form](../../docs/spec/form.md), at
glean's points and under glean's names, so the two answers compare value by
value. The rule it reads them by:

> The present release of a value on a path is the step that ends the
> function's hold on the value's region there: the release that takes the
> function's count of a counted region to zero, the release of an owned
> region's owner, a group free that names the region, or the exit of the
> activation that adopted it.

## What it answers

`present` answers `{:functions [...]}`, one struct per function of the unit,
in the unit's order:

| Key | Value |
|-----|-------|
| `:id`, `:name` | as glean gives them |
| `:released` | `[v p]` for each value `v` whose region the step at `p` releases on some path |
| `:handed` | `[v p]` for each value `v` whose region's last count the step at `p` hands to a callee or to the caller on some path |
| `:held` | `[v p]` for each value `v` whose region the function still counts at the exit `p` on some path, or that the step at `p` leaves where nothing can release it |
| `:over` | `[v p]` for each value `v` whose region the step at `p` releases or hands on where the function holds no count of it |

A point is glean's point, and an exit is the point of its Return or TailCall.
A value is named as glean names it ([names.lisp](names.lisp)): a region made by
a step carries the name of the register that step writes, a parameter's region
the parameter's binding, a capture's region its free name, and a heap
ValueConst `[:const p]`. A region two steps allocate into carries both names,
and its release releases both values.

## Regions

The reader walks each path of the function from its entry. At each point it
knows the region each register and each local slot holds, and for each region
its mode. A region is counted, with the function's count of it, owned by
another region or by the activation, or released. An immediate holds no
region.

| Step | The region it gives | The function's count |
|------|---------------------|----------------------|
| the entry, for each parameter | a region in the parameter's slot | 1, because the callee takes its own reference to each argument |
| the entry, for each capture | a region at the capture's index | 0 |
| ValueConst of a heap value | a region of its own | 0 |
| List, MakeArrayMut, MakeClosure, MaterializeConst, MakeCaptureCell, Freeze, Thaw | the region its static region maps, or a new one | 1 for a new region |
| Call, SuspendingCall, CallArrayMut | a new region for the result, which its static region maps | 1, because the callee hands the caller one reference |
| Eval | a new region for the result | 1, as for a call |
| Const, ValueConst of an immediate, a computed immediate | none | |
| LoadResumeValue, each read [kinds.lisp](kinds.lisp) lists, and every other step that writes a register | a new region | 0, because the step takes no reference |

LoadLocal and StoreLocal move a region between a register and a slot. An
environment index at or past the capture count is the slot that index less the
count names, as glean reads it. A capture cell holds its content's region:
LoadCapture and LoadCaptureCell read the content, StoreCaptureCell and a
StoreCapture into a cell replace it, and LoadCaptureRaw reads the cell.

A static region maps the region its last allocation made, until a DecrefRegion
of it takes the mapping. An allocation into a mapped static region reuses that
region where two allocation steps of the function name the static region,
because the lowerer merged them. Otherwise it maps a new region. A step that
makes a region again while the region it made last is still counted leaves the
old one held at that step, where no register and no slot holds it. The reader
refuses the function where one still does.

## Count steps

| Step | What it does |
|------|--------------|
| IncrefRegion `r` | one more count on the region `r` maps; nothing where `r` maps none |
| DecrefRegion `r` | one fewer count on the region `r` maps, and `r` maps none after it; nothing where `r` maps none |
| IncrefValueRegion `x` | one more count on the region `x` holds, a cell's content for a cell |
| DecrefValueRegion `x` | one fewer count on the region `x` holds, a cell's content for a cell |
| DecrefCellRegion `x` | one fewer count on the region of the cell `x` holds |
| DecrefPostponed `slot`, `route` | one fewer count on the region `slot` holds, then neither slot holds a region |
| AdoptRegion `parent`, `child` | the child's region becomes owned by the parent's, a cell's content for each |
| AdoptCellRegion `parent`, `child` | the same, read through no cell |
| AdoptIntoActivation `child` | the child's region becomes owned by the activation |
| FreeRegionGroup `members` | each member's region is released |
| AssertRegionMatches | nothing |

The step that takes a counted region's count to zero releases it, and each
region it owns, to any depth. A count step on an owned region does nothing.
A step that lowers the count of a region the function holds no count of, or of
a released region, is over.

A hand-over lowers a count as a release does, and the reference goes on to a
callee or to the caller, which releases it. So a hand-over that takes a count
to zero ends the function's hold with no release, and the region is handed.

## Calls

A callee is a native or a closure, and a call does different things for each.
A callee whose register a ValueConst of a native or a closure writes takes that
arm alone. Any other callee takes both arms, as two paths.

| Step | Closure callee | Native callee |
|------|----------------|---------------|
| Call, SuspendingCall | each `[position slot]` of `:moves` hands the callee one count of the region the slot holds, and the slot holds none | nothing |
| TailCall | each argument hands the callee one count of its region, then the path goes to the exit. An argument at a position of `:late-mints` hands on nothing, because the runtime mints the callee its reference | a new region for the result, as for a Call, and the path goes on |
| TailCall with `:defer-callee-release` | the callee's region loses one count | nothing |
| TailCall with `:deferred-release-slot` `r` | the region `r` maps loses one count | nothing |
| CallArrayMut | what a DecrefRegion of `:args-region` does | the same |
| TailCallArrayMut | what a DecrefRegion of `:args-region` does, and the path ends at an exit at its point | what a DecrefRegion of `:args-region` does |

A borrowed argument needs no rule of its own. The step that minted its
reference raised its count before the call, so the closure arm hands on the
minted reference alone. A spliced call hands on nothing, because the callee
takes its own reference to each argument.

## Exits

A Return hands the caller one count of the region of its value. At each exit, a
counted region with a count above zero is held, and a region the activation
owns is released.

## Paths

The state at a point is the region of each register and each slot, and each
region's mode and count. Two paths that reach a point in one state go on as
one, so a loop ends when its states repeat. On a path with no loop, no count
rises past one plus the number of steps that raise a count. So the reader
refuses a function where a count rises past that bound, because only a loop
that retains without releasing raises it there.

The reader refuses a kind the data form does not name and a read of a register
no step writes, as glean does.
