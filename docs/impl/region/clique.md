# What a region-effect declaration buys

<!-- audited: 2026-10-21 -->

What the solver derives from a declared `RegionEffect`.

The argument clique, the hard edges it emits, and the oracle that keeps a
declaration honest.

The declarations themselves are in [effects.md](effects.md).

**What the solver derives.** For an opaque (non-inlined) call, the solver's
baseline assumes every heap argument may be stored into every other. That is
the *arg clique*: mutual may-store edges, each a compile-time `IncrefRegion` of
a heap argument's region at the call site. The target region's free-time
cascade balances that incref *only if the store actually happens*. A clique
incref against a store that never happens never balances — an over-keep
(never a mis-free) that leaks one region per uncounted edge per call.

**The clique is over pairs of ARGUMENTS, never over one argument's own
regions.** A single argument's value may have several source regions, such as
the arms of a branch or a pattern alias into a scrutinee. Those are
*alternatives* for the one value the call receives, never two values one could
store into the other. So a native reached with one heap argument emits no edge
however many regions that argument carries. A store of that argument into
another one is covered by the edges to the *other* argument's regions. Pairing
a flattened region list instead would strand one region per call on a shape
with no second argument at all
([region-native-effect-clique-leak.lisp](../../../tests/impl/region-native-effect-clique-leak.lisp)).
The declared-store path states the same rule as its `j == i` skip
(`record_store_edges`).

Declarations shrink the clique to where it can be real:

- `Immediate` / `Fresh` / `PassThrough` / `Opaque`: no argument is stored —
  **no edges**. (`Opaque` differs from the others only on the *result* side —
  its result is non-fresh, so it still contributes a value-released call-result
  region — but it shares "stores nothing," so it carries no clique.)
- `Funnel`: stores happen but are runtime-counted at the store site — **no
  edges** (an edge would double-count against the single cascade decref).
- `Delivers { args }`: the install into another fiber's signal slot is
  runtime-counted or transient, so — like `Funnel` — **no edges**. What the
  declaration still carries is the *frontier*: escape seeds the listed args on
  its fiber facet, so an installed value is never Owned.
- `Stores { args }`: a directed
  may-store edge from each listed argument's regions to each *other* heap
  argument's regions (the possible in-argument targets). A store into the
  result needs no compile-time edge — the result object's alloc-time scan
  counts it (Rule 5, immutable contents) or the mutable-store funnel does. A
  store into an external structure must be runtime-counted by the native
  itself (`incref_for_escape`); it is invisible to the solver by nature and
  the declaration documents it.
- `Sends { args }`: **no edges**. The send seam retains the message's region
  at runtime (`EscapeSite::ChanSend`). A compile-time edge would double-count
  where it fires, and would fail to fire where the channel's region is not
  nameable at the call site (an upvalue or module-level channel). The
  declaration still seeds the listed args as fiber-frontier crossings for
  escape.
- `Mixed` / `Unknown` (a registered **native** whose store behavior is
  uncounted-or-unexamined): the full mutual clique. A native can
  reach value/ internals and store an argument *uncounted* — invisible to
  both the funnel seam and the solver — so the clique is its only cover.
- **User functions and other non-primitive callees** (the solver's `None`
  case — the callee is not in the primitive effects table): **no edges**.
  This is *not* `Unknown` in disguise. A user function is ordinary Elle code.
  Elle code can store a value into a mutable container *only* through the
  mutable-store funnel (`push`/`put`/`add`/`%put` and friends), which is
  runtime-counted and statically complete (Rule 5). The raw `Value`-bearing
  cells are reachable only inside value/. So every store a user fn performs on
  an argument is already counted at the store site, or by an edge in the
  callee's *own* compilation. A caller-side clique incref is pure redundancy.
  It would leak one region per *alloc-region* heap argument per call, because
  a literal's static slot IS populated, so its `IncrefRegion` is real and never
  balances. A call-result argument is a slot-based no-op
  ([region-userfn-clique-callresult-noleak.lisp](../../../tests/impl/region-userfn-clique-callresult-noleak.lisp)).
  Pinned by [region-userfn-clique-noleak.lisp](../../../tests/impl/region-userfn-clique-noleak.lisp).

Declarations leave the result side unchanged at runtime. The call-result
placeholder and the value-gated `DecrefValueRegion` release (Rule 2) remain the
machinery for every heap-returning effect. `Immediate` calls contribute no
result regions to the walk (the solver's `call_returns_immediate` check, keyed
on this declaration).

**The dispatch pass-through retain, what it is for, and the declarations that
waive it.** `dispatch_native_call` funds the caller's release. A heap result
living outside the call's own minted region gets one owning reference (the
pass-through retain). The caller's `DecrefValueRegion` then balances against it
instead of freeing a region owned elsewhere.

The retain is for a **result**, so it is taken only when the native returns one
— `SIG_OK`. A native that returns a value as a **signal payload** hands it to
the signal machinery, which accounts for it on the path that payload actually
takes:

- A fiber carrier (`fiber/resume`/`fiber/abort`/`fiber/propagate` returning
  their fiber ARGUMENT) is replaced by the child's outcome before any caller
  release runs.
- A suspending payload rides `fiber.signal` under the `SuspendEscape` /
  `EmitEscape` retain, and is released on the resume path.
- An error or halt payload is read through the signal, never through the
  caller's result slot, which the handler stamps `nil`.

No consumer exists for a retain on any of those. Taking one would strand a
region per call: the emitted value of every `fiber/emit`
([region-fiber-install-clique-leak.lisp](../../../tests/impl/region-fiber-install-clique-leak.lisp)),
or a parked-then-discarded fiber's whole region graph (the `multi-resume` /
`yield-discard` oracle probes). The declaration oracle makes the same exemption
for a signal-carrying return, stated on the accounting side.

Beyond that, two `PrimitiveDef` flags declare "the body already supplied that
reference," and the dispatch then skips the retain. Taking it anyway would hand
the caller two references against one release, one stranded region graph per
call:

- **`moves_out`** — the result is an element REMOVED from a container
  argument. The body took the retain in place, necessarily before releasing the
  container's own reference (`arena::pop_with_decref`; the `raw-pop` oracle
  probe pins the double-count).
- **`result_minted`** — compiled code run on the driving VM produced the
  result, through `run_thunk_to_completion`. That code is the module body
  `import/load-file` and `import/load-syntax` run, or the `compile/*-module`
  test loaders' setup accumulator. So the result left that code through the
  return convention already carrying the caller's reference — a **thunk-run
  result**. The claim binds every normally-completing path: a declarant path
  that runs no thunk supplies the reference itself (both `import/load-plugin`
  paths take an explicit `EscapeSite::NativeCallResult` retain).
  Consumed at dispatch only — no solver site reads it. Pinned by the
  `import-result` probe in [oracle.lisp](../../../tests/impl/oracle.lisp).

A thunk-run value *embedded* in a fresh result, rather than returned bare,
needs no flag but still owes the mint's consumption. The fresh container's
alloc-time scan counts the embedding, so the boundary consumes the mint after
the container is built, and the cascade frees the value with it
(`handle_arena_allocs`; the `allocs-result` oracle probe).

What the result side *does* derive is an **alias** fact for the ownership
forest. `Fresh`, `Stores` and `Sends` each claim a heap result in the call's
own minted region, the claim the declaration oracle below checks. `Immediate`
claims no region at all, so none of the four can hand back a value living
inside an *argument*. Every other effect can (`PassThrough` by definition,
`Funnel`'s in-place container return, `Opaque`, `Delivers`, `Mixed`,
`Unknown`), and a non-primitive callee is under no claim whatsoever. For those
the walk records `result ⊒ each argument` (`RegionInfo::opaque_result_aliases`).
A subtree whose member such a result may name is then bound by the root's drop,
or refuses to Shared. That is the result-side analogue of the arg clique, and
the reason `Fresh` is worth declaring even for a primitive that stores nothing
([adopt.md](adopt.md)).

`Funnel` is the one middle case. Its result is arg0 in place or a fresh copy of
arg0, the container either way, never an element interior to it. So it needs no
bound, only the reachability that a read out of that result is a read out of
arg0 (`RegionInfo::funnel_result_containers`). The container READS are excluded
from both relations: their result *is* the interior element, and the borrow
face's own edge records it against the tighter container. That exclusion is
keyed on the read set itself (`CallClassification::container_read_funnels` —
`get`/`first`/`rest`/`pop` and their `%`-op peers), not on the declared effect,
so it holds however each read is declared.

**Hard edges: how a may-store edge is emitted.** An edge's compile-time
incref is keyed by the *source* region. For a region minted by an alloc
opcode the static slot resolves at runtime and a slot-based `IncrefRegion`
is correct. For a **call-result placeholder** the slot is never populated,
because only alloc opcodes record region mints in the activation map. So the
slot-based incref would be a silent no-op, while the edge's balancing decref,
the store target's free-time cascade, is real. If the store happened, the
cascade would steal a live reference: the call-result-arg clique UAF
([region-native-clique-callresult-uaf.lisp](../../../tests/impl/region-native-clique-callresult-uaf.lisp)).

Who recorded the edge decides how it is emitted:

- Edges recorded at a **native call site with a declared uncounted-store
  effect** (`Stores` / `Mixed` / `Unknown`, a known primitive) are **hard
  edges**: the store is real or must be presumed real. For a call-result
  source the lowerer increfs by *value*. It loads the argument from its binding
  slot and retains the runtime region the value actually lives in, so the
  cascade's decref is balanced.
- An **opaque user-fn call site** (callee effect `None` — not a registered
  primitive) records **no clique edges at all** (see the user-functions
  bullet above). Every store a user fn performs on an argument goes through
  the runtime-counted mutable-store funnel, or is counted by an edge in the
  callee's own compilation, so no caller-side edge is needed. A slot-based
  edge would leak one region per alloc-region argument per call, and a
  value-based one would leak the call-result case too (the funnel already
  counts the store). One residual remains. A user fn that dispatches an
  uncounted-store native over a *call-result* argument is a UAF candidate
  when the dispatched native is not statically resolvable at the call site.
  There `call_effect` returns `None`, so no hard edge is recorded.
  When the native *is* resolvable in the callee's own compilation it is a
  hard-edge site like any other `Stores`/`Mixed`/`Unknown` native call, and
  the value-based incref above covers the call-result source there. (A
  seam-counted native — `Sends`, `Funnel`, `Delivers` — has no such residual:
  its store is counted at the seam however the callee is reached.)

**The declaration oracle.** A declaration is a soundness claim, so it is
checked, forever. In debug builds `dispatch_native_call` compares the declared
effect against `region_of(result)` after every native call that completes
normally:

| Effect | A heap result must live |
|--------|-------------------------|
| `Immediate` | nowhere: the result has no region |
| `Fresh` / `Stores` / `Sends` | in the call's own minted region |
| `PassThrough` | anywhere *but* the call's own minted region |
| `Funnel` / `Mixed` / `Unknown` / `Opaque` / `Delivers` | anywhere: no check |

An `Opaque` or `Delivers` result may live anywhere; that is the point of both
variants. A violation panics deterministically, naming the primitive.
Signal-carrying returns (error/yield payloads) are exempt —
their payloads ride the signal machinery's own accounting. The oracle
cannot see the store side (that is the mutable-store funnel's and
guardfree's territory); it polices the result claim on every debug run, so
a mis-declared primitive cannot survive the suite.

Declaring less than is true (`Mixed` when the native is really `Fresh`)
costs precision and tolerates the clique leak; declaring more than is true
is a correctness defect the oracle converts into a deterministic panic.
An unexamined primitive stays `Unknown`; after reading it, declare the
strongest claim that holds on every path — `Mixed` if none does — one
primitives table per commit.

**The return-type declaration (`RetType`).** Beside its effect, a primitive may
declare a statically-known return type (`PrimitiveDef::ret`). Type inference
consumes it for the `type-of` dispatch prune, and the ownership forest consumes
it at two points:

- A `Funnel` store's **container** classification. A
  `MutableArray`/`MutableStruct` container retains the stored value's region,
  so the walk recovers a containment edge. A `@string`/`@bytes` container
  copies bytes and retains nothing.
- The **fiber-member refusal**. The result region of a call whose declared
  type is `Fiber` (`fiber/new`) is recorded in
  `RegionInfo::fiber_result_regions`, and no region-rooted cut ever adopts it
  ([adopt.md](adopt.md)).

A `RetType` claim must hold on **every** normally-completing path. A nullable
result (`fiber/child`, which returns nil before any resume) declares
`Unknown`, never the heap type, or the prune would cut a live `nil` dispatch
arm.
