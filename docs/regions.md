# Region-Based Memory Management

<!-- audited: 2026-09-22 -->

Elle frees memory at compile-time-known program points: no tracing collector,
no liveness scan, and no GC pause. There is no `Rc<Value>` either — a `Value`
is a `Copy` 16-byte tag+pointer.
Every value is born in a *region* — a set of pages. The region frees at a point
the compiler named, either when its count reaches zero or when its owner frees,
and the regions its values referenced are released in turn. You never call
`free`.

The model is the Tofte–Talpin region calculus for immutable values, completed
with per-region reference counting for the one case TT cannot express —
mutation — and an ownership forest that frees a proven-owned group of regions,
cycles included, with its owner. There are exactly two measures: a region
implementation is **correct** (never reads freed memory, never frees live
memory, never leaks past a value's last reference) and then **optimal** (how few
regions, how little RSS). Optimization may never buy performance with
correctness.

This file is the **index** to the region documentation, split by audience.

## For programmers writing Elle (the consumer model)

Read these to write code that is sympathetic to the memory system and to get the
semantics and performance you want from it:

| Topic | Content |
|-------|---------|
| [regions/semantics](regions/semantics.md) | The model you write against: TT for immutable values, counts for mutation, ownership for proven-owned groups; what escapes stays alive; which cycles free and which leak; regions are not scopes, and their ids are flat. |
| [regions/lifetime](regions/lifetime.md) | How long things live: the naive user model, why your "constants" are immutable but not eternal (re-materialized per `eval`), and what is true after your program ends. |
| [regions/performance](regions/performance.md) | Where region performance comes from: merging, the per-region cost model, and the habits that make code cheap. |

## For implementors of the region system

Start at [impl/memory.md](impl/memory.md). It states the mission and the settled
invariants, and it maps every implementor document: the correctness rules, where
each release is placed, the runtime substrate, bindings and cells, native calls,
and the instruments that tell correct from broken.
