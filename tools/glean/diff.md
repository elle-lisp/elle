# The release diff

<!-- audited: 2026-11-24 -->

Where the present releases end the hold on a value early, late or never, against the liveness the spec derives from glean's facts.

`diff` ([diff.lisp](diff.lisp)) reads a unit in
[the data form](../../docs/spec/form.md). It compares the liveness the spec
derives from [glean](overview.md)'s facts of each function with the holds
[the present releases](present.md) give, by this rule:

> At each step other than a count instruction or a step that only moves a
> value (LoadLocal, StoreLocal, LoadCapture, LoadCaptureRaw, Const,
> ValueConst), a value whose region the function counted at its birth is
> early where the spec keeps it live and its region has ended, late where the
> spec does not keep it live and the function still holds its region, and
> none where the function still holds it at an exit.

## What it answers

`diff` answers `{:functions [...]}`, one struct per function of the unit, in
the unit's order. Each key below holds a value only where present's
`:counted` holds it.

| Key | Value |
|-----|-------|
| `:id`, `:name` | as glean gives them |
| `:early` | `[v p]` for each value `v` the spec keeps live at the step at `p`, where present's `:ended` holds `[v p]` |
| `:late` | `[v p]` for each value `v` the spec does not keep live at the step at `p`, where present's `:holding` holds `[v p]` |
| `:none` | `[v p]` for each pair of present's `:held` |

## The spec side

The spec keeps `v` live at `p` where `live-as(v, p)` holds
([liveness](../../docs/spec/liveness.md)). The diff derives it from glean's
facts in three steps. `returning` ([return](../../docs/spec/return.md)) adds
the use at each exit, `with-captures` ([capture](../../docs/spec/capture.md))
makes each late capture a store, and `live-all` runs liveness under every wait
rule. Glean renames a value at each pass, and present names a region by the
value that made it. `live-as` keeps a value live while a block parameter that
carries it is live, so the two names agree with no walk over `pass`.

Glean's noise changes no fact, so the diff hears it and drops it
([the census](census.md)).

## The steps it judges

The diff judges every point but a count instruction
([kinds.lisp](kinds.lisp)) and the six kinds that only move a value. The
present lowering loads a value before it releases it, and clears the slot
after, so a hold across those steps costs nothing. An exit is no step, so
`:none` alone reads it.

## What it cannot see

Glean gives no `read` for an unruled kind, such as Get, or for a call of a
native with no `:spec` ([glean](overview.md)). So the spec keeps no container
live for a value read that way. A release of the container before the value's
last use does not read early, and a hold until that use reads late.

A capture, a heap constant and a read begin with no count, so a count step on
one of them only ends a borrow, and the diff judges none of them.
