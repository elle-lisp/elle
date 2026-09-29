# Region performance — merging and the cost model

<!-- audited: 2026-09-29 -->

This is the consumer's view of where region performance comes from and what you
can and cannot affect. The implementor's account of the page pool is in
[docs/impl/region/model.md](../impl/region/model.md) and the merge predicate in
[region/merging.md](../impl/region/merging.md).

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
reclaims them when a region's last reference dies. It can return excess pages to
the OS under pressure. One region per value, unmerged, would claim a page per
allocation — correct but expensive.

**Merging** collapses two regions into one when they provably share the same
demise point and the same cross-region reference topology. The merged region is
freed at the same moment its constituents would have been. So merging never
changes *when* memory is reclaimed — only *how many* distinct regions and pages
the run uses. This is where the bulk of region performance comes from: values
with coincident lifetimes share a region and amortize the per-region page cost.

Merging is monotonic: once two regions merge they stay merged. The compiler only
merges when it can prove lifetimes coincide. So a failure to merge costs
performance and **never** correctness. At worst a value's region is kept as long
as its own last use rather than shared, never freed too early.

## Recycling a page is free

A region's pages go back to a per-thread cache when the region dies, and the
next region claims them from there. Both directions are a free-list operation:
no system call, and nothing reads or writes the page. So a short-lived region
costs what it writes and nothing more, which is what keeps the one-region-per-
value baseline affordable.

The cache is bounded. Past that bound a released page is unmapped instead of
kept, which is where memory returns to the OS. A page inside the bound stays
resident because it is about to be handed out again. The implementor's account
is in [impl/region/model.md](../impl/region/model.md).

## Values that share a region

A region owns at least one page, and most values are much smaller than a page.
So the memory a structure holds depends on how many regions its values occupy,
not only on how many values it has. Elle puts values in one region wherever
they provably die together. The implementor's account is
[impl/region/colocation.md](../impl/region/colocation.md).

A rest list is one region, however many arguments it collects:

```lisp
(defn collect [& xs] xs)

(def before (arena/region-count))
(def collected (collect 1 2 3 4 5 6 7 8))
(assert (= (- (arena/region-count) before) 1)
        "eight collected arguments occupy one region")
```

A value you push into a container you built, and never take anything out of,
is born in the container's region:

```lisp
(defn pairs [n]
  (def out @[])
  (var k 0)
  (while (< k n)
    (push out [k k])
    (assign k (+ k 1)))
  out)

(def before-pairs (arena/region-count))
(def built (pairs 1000))
(assert (= (- (arena/region-count) before-pairs) 1)
        "a thousand pushed pairs share the container's region")
```

The same builder with a `pop` anywhere in it keeps one region per pair. A
region cannot tell which of its values died, so a container that both takes and
loses values would keep every value it ever held until the container dies.
Elle refuses to colocate there, and your queue stays as small as what it holds.

## A call into a variadic stdlib operator allocates

The arithmetic and comparison wrappers are ordinary variadic Elle functions:
`+` is `(defn + [& args] (letrec [go …] (go 0 args)))`. Calling one with two
arguments builds the rest list and the `letrec` closure that the definition
asks for. The whole rest list is one region, and the closure is another, and
each region owns a page. `(+ a b)` therefore claims two pages, where
`(%add a b)` is one VM instruction and claims none.

That is the price of the wrapper's polymorphism, its runtime type checks, and
its `:error` signal. It is also the reason
[docs/intrinsics.md](../intrinsics.md) tells you to reach for `%add` in a hot
loop and for `+` everywhere else.
[tests/elle/region-page-recycle.lisp](../../tests/elle/region-page-recycle.lisp)
pins the per-call page count, so the number above is measured rather than
asserted.

## Passing arguments costs one pass over them

A call's region bookkeeping is linear in the number of arguments. Each argument
is classified once — which region it lives in — and retained or released once.
Nothing compares arguments against each other.

This holds for every calling convention, and the one that has to work hardest
for it is the tail call to a variadic callee. There the caller's reference to
each argument *moves* to the callee, the rest arguments land in a collected
list that took its own reference, and the moved-in reference is surplus. Only a
value that appears exactly once across the whole argument list may be released.
One that appears twice shares a single moved reference, and a second release
would free it out from under a live use. So the release step needs each
value's occurrence count, and it takes them from one counting pass, not from
comparing every argument with every other. `(apply f xs)` in tail position over
a 40000-element `xs` is a 40000-step operation, not a 1.6-billion-step one
([tests/elle/apply-tail-linear.lisp](../../tests/elle/apply-tail-linear.lisp)).

## What you can do

You do not control merging directly, but the same habits that make code clear
also make it cheap here:

- **Prefer immutable data.** Immutable values are fully statically tracked, merge
  freely, and never participate in the one leaking case (mutable cross-region
  cycles — see [semantics.md](semantics.md)).
- **Let values die where they're last used.** A value that escapes (into a
  long-lived container, a closure, a returned result) keeps its region alive for
  as long as it is reachable. That is the cost of escaping, and it is the same
  cost any memory system pays. Short-lived intermediates are freed promptly with
  no help from you.
- **Build a collection where it is born.** Push fresh values into a container
  the same function created, and hand the container back when it is full. The
  values then share its region. Take values out of a different container than
  the one you fill.
- **Don't hand-pool or intern expecting a win.** Re-materializing a literal per
  use is the correct baseline; sharing it is an optimization the compiler may
  apply. Any sharing you build yourself must keep the value reachable for as
  long as you use it (ordinary RC). Never stash it somewhere "permanent."
