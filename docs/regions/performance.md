# Region performance — merging and the cost model

This is the consumer's view of where region performance comes from and what you
can and cannot affect. The implementor's account of the page pool and the merge
predicate is in [docs/impl/region-model.md](../impl/region-model.md).

## Correctness first, performance second — never the trade

Memory management here is **correct or it is broken**; performance is a separate
axis layered on top and may never buy speed with correctness. The baseline is one
region per value, each freed precisely: correct, and slow (a page per
allocation). Everything that makes it fast — chiefly *merging* — is an
optimization that preserves the same observable freeing behavior. So you never
have to reason about a fast-but-leaky mode versus a slow-but-safe mode: there is
one behavior, and the optimizations are invisible except in time and memory.

## Merging amortizes the per-region cost

A per-thread page pool with size classes hands pages to regions on demand and
reclaims them when a region's last reference dies; it can return excess pages to
the OS under pressure. One region per value, unmerged, would claim a page per
allocation — correct but expensive.

**Merging** collapses two regions into one when they provably share the same
demise point and the same cross-region reference topology. The merged region is
freed at the same moment its constituents would have been, so merging never
changes *when* memory is reclaimed — only *how many* distinct regions and pages
the run uses. This is where the bulk of region performance comes from: values
with coincident lifetimes share a region and amortize the per-region page cost.

Merging is monotonic: once two regions merge they stay merged. The compiler only
merges when it can prove lifetimes coincide, so a failure to merge costs
performance and **never** correctness — at worst a value's region is kept as long
as its own last use rather than shared, never freed too early.

## What you can do

You do not control merging directly, but the same habits that make code clear
also make it cheap here:

- **Prefer immutable data.** Immutable values are fully statically tracked, merge
  freely, and never participate in the one leaking case (mutable cross-region
  cycles — see [semantics.md](semantics.md)).
- **Let values die where they're last used.** A value that escapes (into a
  long-lived container, a closure, a returned result) keeps its region alive for
  as long as it is reachable; that is the cost of escaping, and it is the same
  cost any memory system pays. Short-lived intermediates are freed promptly with
  no help from you.
- **Don't hand-pool or intern expecting a win.** Re-materializing a literal per
  use is the correct baseline; sharing it is an optimization the compiler may
  apply, and any sharing you build yourself must keep the value reachable for as
  long as you use it (ordinary RC), never stash it somewhere "permanent."
