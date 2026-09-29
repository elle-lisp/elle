# Colocation

<!-- audited: 2026-09-29 -->

Which values share a region, the one rule that keeps sharing sound, and the patterns that apply it.

A region owns at least one base page, and a heap object takes one 128-byte
slot. A region that holds one object therefore wastes most of its page, and a
program that keeps ten thousand such regions alive keeps ten thousand pages.
Colocation puts several values in one region, so they share its pages. This
document owns the argument for when that is sound, what it costs, and how each
pattern pays for itself. [merging.md](merging.md) is the static form this
document generalizes; [model.md](model.md) is the page layout.

## Two costs, two gauges

- **Retained footprint.** Each live region holds at least one page.
  `arena/region-count` and `arena/bytes` measure it. A retained structure of
  small values built one region per value costs a page per value.
- **Churn.** Each region minted claims a page from the pool, even when the
  region dies at once. `arena/page-claims` measures it
  ([diagnostics.md](diagnostics.md)). A claim is a free-list pop, but a
  region also costs its `RegionEntry` and its edge tables.

A pattern states which cost it removes. The gauge file
[region-colocation.lisp](../../../tests/elle/region-colocation.lisp) measures
every pattern in both dimensions, beside a discriminator that proves each gauge
moves.

## The join rule

To **join** a region is to allocate into a region that already exists instead
of minting a fresh one. One rule makes every join sound:

> A join takes one reference on the region it joins, and the joining site
> releases that reference exactly where it would have released a fresh region.

A fresh mint hands its site one reference, the birth count. A join hands its
site one reference too. So every release rule in [rules.md](rules.md) reads the
same count it always read, and no release moves. The region reaches zero only
when every site that joined it, and every holder of any value in it, has let
go. Sharing a region can keep a value alive longer. It cannot free one early.

Four guards make that true at runtime. Each one is a refusal, and each refusal
falls back to the fresh mint, which is always legal.

1. **A join refuses an `Owned` region.** An `Owned` region has no count to take
   ([ownership.md](ownership.md)), so a join into it would hold nothing. The
   site mints fresh instead. A partner with no region (an immediate, the empty
   list) also mints fresh.
2. **A joined region is never adopted.** Adoption moves a region from
   `Counted` to `Owned` and consumes its whole count. A joined region's count
   belongs to many sites, and an adopt would hand all of them to one owner. So
   `adopt_region` and the owner-node adopts leave a joined region `Counted`.
   The compiler never emits such an adopt for a region it joins; the runtime
   refusal is for the scope arena, where the compiler cannot see the join.
3. **A reference from a region to itself is never counted.** The allocation
   scan, the free cascade, and the mutable-store funnel all skip it, on the add
   half and on the remove half alike. Without this, a value pushed into a
   container in its own region raises a count that no cascade lowers.
4. **A call gives back a join it did not use.** A native call's result region
   is minted before the native runs. When the call joined a region and the
   returned value, result or signal payload, lives elsewhere, the dispatcher
   releases the join's reference. The fresh-mint twin of this rule is the
   unmaterialized-id recycle ([model.md](model.md)).

The declaration oracle ([effects.md](effects.md)) reads "the result lives in
the call's own region" as "fresh". After a join, a pass-through result can live
in that region too, so the oracle does not check the `PassThrough` claim on a
joined call.

### Merge and join

A merge ([merging.md](merging.md)) collapses static slots. The child's release
is suppressed, the self-edge increment is dropped, and the root's one
`DecrefRegion` frees the tree. That needs a static proof that the child's
release is the parent's, and it pays no runtime count traffic.

A join needs no such proof, because each site keeps its own release. It is the
form for a partner only the runtime can name: the region a container lives in,
or a scope's arena. Both forms stay. Prefer the merge where both ends are
static slots with one demise.

## What bounds a colocation

A join is sound whatever it joins. What a pattern must argue is that its
**over-keep is bounded**.

A region's count is per region, not per object. When one value in a shared
region dies, the region cannot tell, and the bump allocator never reuses a
slot. So a dead value's slot stays until the whole region dies. That is free
when the members die together, and it is unbounded when a long-lived region
keeps taking members that die before it.

Every pattern therefore names one of three bounds:

- **Containment.** Every member is reachable only through the region's root, so
  no member dies before the root. Nothing becomes garbage early.
- **Coincidence.** The members die at the same program point.
- **Scope.** The region dies when a closed computation ends, so its garbage is
  bounded by what that computation allocates.

The shape every pattern refuses is **churn**: a long-lived container that both
takes and loses members. A queue, a worklist, a mailbox, or a cache updated in
place would grow by one slot for every value it ever held, where one region per
value keeps it at its live set.

## The patterns

| Pattern | Partner | Bound | Realized by |
|---------|---------|-------|-------------|
| Construction | the region the builder was handed | containment | natives, the rest list, `&keys`, errors, syntax |
| Containment | the parent aggregate | containment | the `%pair` builder merge |
| Cycle | any member of the cycle | coincidence | the `letrec` merge |
| Scope arena | the scope's arena | scope | the syntax working arena, macro expansion |
| Append-only container | the container | containment | the append seed |
| Accumulator | the previous accumulator value | containment | open |
| Sibling | a value with the same demise | coincidence | open |

### Construction

One Rust operation builds a whole structure, and every part of it is reachable
only through the value it returns. The operation allocates every part into one
region.

- A native allocates through the `Alloc` its call was handed, so its result and
  every member share the call's region ([ctx.md](ctx.md)). A helper building
  part of a result takes the caller's `&Alloc` and never mints.
- A rest parameter's list is minted once and every cons is born in it. The
  first cons's `rest` is the empty list, and each later cons points at the
  previous one in the same region, so no edge is counted. The head's release
  frees the list.
- A `&keys` or `&named` struct and a rich error are one region each
  ([errors.md](errors.md)).
- A closure's captured environment and an immutable aggregate's payload are
  `RegionSlice`s in the object's own pages ([model.md](model.md)).

### Containment

A child built only to be stored into one parent shares the parent's region. The
builder-idiom merge realizes it for a `%pair` whose child and parent are both
local ([merging.md](merging.md)).

Open widenings, each needing a probe first:

- A parent built by a `Fresh` constructor native (`[…]`, `{…}`, `list`), whose
  children are call results. The nested literal `[[1 2] [3 4]]` is three native
  calls and three regions.
- A parent that is returned or captured. The child still dies inside it.

### Cycle

The members of a reference cycle can only die together, so they share one
arena. The `letrec` closure-cycle merge realizes it ([letrec.md](letrec.md)).

### Scope arena

A closed computation whose result is copied out allocates everything into one
arena, and the scope's close frees the arena.

- The syntax working arena is one region per compilation unit
  ([../syntax.md](../syntax.md)).
- A macro expansion is one arena. `begin_macro_scope` mints the arena and holds
  one reference on it. While the scope is open, every value region the VM mints
  joins the arena instead: an allocation slot, a native call's result, and an
  environment value. The wrapped arguments are born in it too. The close drops
  the scope's reference and balances the unexplained ones exactly as it always
  has ([rules.md](rules.md)). A mint that must outlive the scope does not go
  through the VM's value mints: a process root, the root region, and a code
  payload region each mint fresh.

The scope arena keeps a transformer's garbage until the expansion ends, which
is the bound. It removes nearly every page claim a transformer makes.

### Append-only container

A value pushed into a container is born in the container's region, when the
compiler proves nothing ever takes a value out of it. The seed is
`region::infer::join`, and it admits a push site when all of these hold:

1. The site is an append store (`push`, `%array-push`), and the pushed value is
   a fresh allocation: a local allocation, or a `Fresh` native's call result.
2. The pushed value is used only by that push. No user binding holds it, and it
   is not returned.
3. The container is an immutable binding in the same function, bound to a fresh
   mutable-container constructor.
4. Every other use of the container is a read: the container argument of
   another append, an argument to a native that stores nothing and removes
   nothing, or the function's result. A removal, an overwrite, a store of the
   container elsewhere, a capture, and passing it to a function all refuse
   the whole container.

The lowerer emits `JoinRegion { region, partner }` immediately before the
pushed value's allocation, with `partner` a read of the container. The runtime
records a pending join on the fiber, and the very next value mint consumes it,
taking the join's reference if the partner's region is live and `Counted`. A
pending join holds no count, so one left unconsumed costs nothing.

Both regions leave the ownership forest: the container and the pushed value are
added to the not-ownable set, and no merge seed may use them. They reclaim on
the reference-count baseline.

The container may be returned. Joins come only from the function that built
it, so after it returns its region takes no more members.

### Accumulator — open

`(assign acc (pair x acc))` in a loop builds a list one region per cons. The new
cons joins the region of the previous value, which it contains. The bound needs
every store into `acc` to be that join or a value unrelated to `acc`'s region:
a store of `(rest acc)` makes the popped cons garbage in a live region.

### Sibling — open

Two local values with the same demise and no edge between them share a region.
[model.md](model.md) names it sibling page-amortization. It needs the
coincidence proof the merge predicate's gate 6 already gives.

## Applying a pattern

A new colocation states three things: the partner, the bound, and the gauge.

**In Rust**, take the `Alloc` you were handed and allocate every part of the
structure through it. Mint only at a boundary that has no caller-supplied
region. For a closed computation, open a scope whose close is the only way to
end it, the way `MacroScope` does.

**In the compiler**, write a seed beside `region::infer::join`. Name the
allocation site and the binding that supplies the partner, and prove the bound
from escape facts and uses. Add both regions to the not-ownable set. The
lowerer's join emission and the runtime need no change.

**Every pattern lands with**, in this order:

1. a probe in [region-colocation.lisp](../../../tests/elle/region-colocation.lisp)
   that measures regions and page claims per structure and fails before the
   change;
2. a `--trace=guardfree` fixture in the `region_*_uaf` family that reads a
   member after the partner's other holders let go;
3. the oracle, the guardfree family, and the corpus smoke
   ([../assessment.md](../assessment.md)).

## Finding the next target

The gauges say how much a shape costs. To find which code claims the pages,
build the `profiling` profile and count calls to `RegionPool::add_page` by
caller:

```sh
make page-claims ARGS="tests/elle/traits.lisp"
```

The target runs the binary under callgrind with the callers of `add_page`
separated, and [scripts/page-claims](../../../scripts/page-claims) ranks the
call paths. The top entries are the next patterns to realize.
