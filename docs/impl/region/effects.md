# Native region effects: declared, not guessed

Implementation-facing: how each primitive declares its region behavior, what
the solver derives from the declaration, and the oracle that keeps a
declaration honest. This is the native-call analogue of Rule 2's opaque-call
exception and Rule 5's escape list (see [rules.md](rules.md)),
made explicit per callee instead of assumed worst-case for all.

Every primitive declares its region behavior in its `PrimitiveDef` as a
`RegionEffect`:

- **`Immediate`** — the result is always an immediate (int, float, bool,
  nil, keyword); no heap value is returned and no argument is stored
  anywhere that outlives the call.
- **`Fresh`** — a heap result is freshly allocated in the call's own result
  region; no argument is stored anywhere *outside* the result. The result
  MAY embed references to the arguments (`pair`, `list`, struct
  constructors): those are counted by the result object's alloc-time scan
  and released by its free-time cascade (Rule 5, immutable contents) — no
  compile-time **RC** edge needed, which is exactly why an embedding constructor
  is `Fresh` and not `Stores`. The **ownership forest**, however, must know
  *which* arguments the result embeds, so it does not judge an embedded value
  externally unique and adopt it while the escaping result still references it
  (the captured-trait-table-into-an-escaping-value shape). `Fresh` alone is too
  vague — `popn` is `Fresh` yet embeds none of its args — so a `Fresh` native
  that embeds an argument names it in `PrimitiveDef::embeds`, and the region walk
  records a `result ⊇ arg` containment edge (no `IncrefRegion`; the alloc-scan
  already counts the RC) for the forest to read (`with-traits`'s `&[1]` — its
  `traits` side-field embed). (An immediate result — e.g. a nil
  error-path return — is always permitted; the claim constrains the heap
  case. Same for the variants below. Note:
  `Fresh` therefore does NOT mean "no new holder of the argument's
  region" — uniqueness inference consults the call-result flow AND the `embeds`
  containment, not this declaration alone.)
- **`PassThrough`** — a heap result is one of the arguments or a value
  living in an argument's region (or a value with no region, e.g. an
  immediate); no argument is stored. The dispatch pass-through retain
  (Rule 5's "new reference")
  hands the caller its counted reference.
- **`Stores { args }`** — the listed (0-based) arguments may be stored
  into another argument or an external structure *without* a runtime
  count the solver can rely on. A heap result is fresh (a native that
  both stores and returns non-fresh is `Mixed`). Stores into the result
  are `Fresh` (alloc-scan counted, above); stores through the mutable-
  store funnel (`push`/`put` and friends) are runtime-counted too — a
  funnel-storing native declares `Funnel`, below. This is *containment*:
  the stored value goes into a structure that stays within the caller's
  ownership subtree (`ffi/callback` stores a closure into a C function
  pointer), so it is **not** a fiber-frontier crossing and seeds nothing
  for the ownership forest.
- **`Sends { args }`** — like `Stores`, but the listed arguments cross a
  **fiber boundary**: they are handed to another fiber (`chan/send`'s
  message rides the channel to the receiving fiber, by pointer under the
  single-threaded scheduler). The solver does the *identical* edge and
  lifetime accounting as `Stores` (the stored arg is increfed and kept
  alive in the channel buffer until received) — but where a `Stores` target
  is an in-heap container whose free-time cascade balances the incref, a
  channel buffer is **external** to the region system, so nothing cascades
  it. The message is a genuinely-Shared (no-bounded-dominator) region, and
  its incoming count is maintained the way § class 7 of
  [ownership.md](ownership.md) prescribes: the send bumps it (this
  edge), and the **receive lowers it** — `chan/recv` / `chan/try-select` /
  `chan/wait-ready` each decref the message's region as it leaves the buffer
  (`release_received_message`, guarded by `value_in_region_store` so a
  cross-thread message on a foreign heap is left to that heap's accounting).
  Without the receive-side release the send-site incref never balances — one
  leaked region per send/recv cycle (`tests/elle/region-chan-send-recv.lisp`,
  and the `chan-send-recv` probe in `tests/elle/oracle.lisp`). The
  fiber-frontier *escape* of the message is the escape analysis's fiber/send
  facet (`hir::escape`)
  — the **send** half of the ownership forest's fiber-facet Shared seed. The
  distinction from `Stores` is exactly the *frontier*: a value sent on a
  channel leaves the fiber and cannot be Owned, where a contained value
  stays inside the subtree. A heap result is fresh (`chan/send` returns a
  fresh `[:ok]`), so the oracle's result-side check is identical to
  `Stores`. `chan/send` is the sole declarant today.
- **`Funnel`** — every argument store goes through the mutable-store
  funnel (arena.rs `push_with_incref`-style, runtime-counted — the
  statically-complete store seam; the same seam records the *outgoing edge* the
  free-time walk consumes, [ownership.md](ownership.md) § The outgoing edge
  table), and the result may be fresh
  OR pass-through (`put` on an immutable struct copies; on a mutable one
  returns the container). No solver edges: a compile-time clique incref
  would double-count the funnel's runtime incref against the container's
  single free-time cascade decref — one leaked region per stored value
  per call, which is exactly why `Funnel` emits no clique edge (the
  `put`/`push` store probes in `tests/elle/oracle.lisp` pin the seam
  reclaiming). No
  result-side oracle constraint (either freshness is legal), exactly as
  `Mixed`. It exists so a funnel-storing op is not forced into
  `Mixed`'s clique.
- **`Opaque`** — examined, and confirmed to store **no** argument (every
  argument is read or copied out — into a Rust `String`/`Vec`, the kernel,
  a fresh structure — never retained uncounted in another argument or an
  external structure), but the result is neither always-fresh nor
  always-pass-through: it lives in neither the call's own region nor an
  argument's (e.g. a value minted on the scheduler heap and delivered by a
  fiber resume — `subprocess/exec`'s `{:pid :stdin … :process}` struct).
  This is `Mixed` **minus the store**: **no arg clique** (nothing is stored,
  so the mutual may-store edges would only leak — exactly the gap that forces
  a no-store, opaque-result, multi-heap-arg primitive into a leaking `Mixed`),
  and **no result-side oracle constraint** (the result may live anywhere). It
  is the variant that separates the two properties `Mixed` conflates — an
  uncounted *arg store* (which justifies the clique) from a merely *non-fresh
  result* (which does not). Declare `Opaque`, not `Mixed`, for a primitive
  that returns an opaque result but stores nothing.

  **A read-only trait dispatcher is `Opaque`.** A primitive that resolves its
  work through the trait table (`has?` → `Collection:has?`) has an unbounded
  result: `with-traits` may replace the protocol with a user closure returning
  anything, so neither `Immediate` nor `Fresh` holds on every path. The *store*
  side is bounded regardless — the built-in method reads and returns a bool,
  and a user closure is ordinary Elle code, which stores only through the
  runtime-counted mutable-store funnel (the same argument the user-function
  case below makes). Two properties, two answers: unbounded result, no store —
  `Opaque`. Declaring `Mixed` there buys nothing and costs one never-balancing
  `IncrefRegion` per heap-argument pair per call
  (`tests/elle/region-has-clique-leak.lisp`).

  The sequence reads and conversions are the same shape and take the same
  declaration: `first`, `second`, `rest`, `->array` and `->list` each resolve
  through `Sequence`/`Collection`, and each hands back an element of arg0, arg0
  itself, or a fresh collection built from it — unbounded on the result side,
  storing nothing. Their clique is empty either way (each takes one heap
  argument, and the clique is over *pairs* of arguments), so what a `Mixed`
  declaration would cost them is on the ESCAPE side, which reads the same
  declaration: `Mixed`/`Unknown` seeds every argument on escape's **store**
  facet ([../escape.md](../escape.md)), and a region escaping by a facet other
  than return keeps the conservative baseline at every mechanism gated on
  `sole_frame_held_regions` — the branch-arm release window among them. A
  declaration is therefore a claim about escape as much as about edges, and the
  strongest true one is what a read-only dispatcher owes
  (`tests/elle/region-sequence-read-effect.lisp`).

  **A native that re-enters the VM is `Opaque` on the store side.** `vm/query`
  selects an operation by a runtime string; `compile/run-on` dispatches a
  caller-supplied closure on a chosen tier; the `compile/*-module` loaders run a
  compiled thunk. None of that reaches the store side: each copies its arguments
  out (a Rust `String`, a cloned `Syntax`) and the Elle code it re-enters can
  store only through the runtime-counted funnel, exactly as an opaque user fn
  can. The *result* is what the re-entry makes unbounded, so the answer is again
  `Opaque` — and the obligation it carries is on the dispatch, not the primitive:
  an operation added behind one of these gateways that RETAINS an argument past
  the call invalidates the declaration and must move it back to `Mixed`
  (`tests/elle/region-query-clique-leak.lisp`).
- **`Delivers { args }`** — the listed (0-based) arguments are handed to
  **another fiber** by installing them in its signal slot, and the result is
  unbounded. The fiber value installers are the declarants: `fiber/resume`'s
  resume value, `fiber/abort`'s and `fiber/cancel`'s error payload, and
  `fiber/emit`'s emitted value. Each carries both of the properties `Mixed`
  conflates, so `Delivers` answers them separately:

  - **The argument side is `Funnel`'s answer — no clique.** Every install seam
    accounts for its own reference at runtime. An install that OUTLIVES the call
    takes the park-retain and records the `fiber → signal` outgoing edge
    (`record_terminal_signal_park`: the hard kill's, and the completing resume's
    step-6a park), so the fiber's free-time signal scan balances it. An install
    the next step CONSUMES is a transient handover — `do_fiber_resume_single`
    takes the value straight back out of the slot and hands it to the resumed
    frame as a borrowed operand, while the caller's own frame stays parked
    underneath the resume, holding the reference that keeps it alive. A
    compile-time incref would double-count the first against its single cascade
    decref and never balance the second, which is exactly the arg-clique leak
    (`tests/elle/region-fiber-install-clique-leak.lisp`).
  - **The argument side is also `Sends`'s answer — a frontier crossing.** The
    value goes to a fiber this activation does not bound, so escape seeds each
    listed argument on its **fiber** facet (`hir::escape`), never the store
    facet: an installed value is never Owned by the installing activation's
    subtree.
  - **The result side is `Opaque`'s answer — unbounded.** A resume hands back
    whatever the resumed fiber yields or returns, minted on that fiber's own
    activation; an abort of a dead fiber hands back a value read out of the fiber
    argument. So the result may live anywhere, the declaration oracle makes no
    result-side check, and the walk records `result ⊒ each argument`.

  The distinction from `Sends` is *who counts the store*: `chan/send` leaves its
  message in a channel buffer external to the region system, which nothing
  cascades, so the send-site incref IS the message's reference and `chan/recv`
  lowers it. A fiber's signal slot is not external — it is a scanned field of a
  region-managed fiber object — so the seam counts it and a solver edge would
  double-count.

  **What a delivered value still owes is owed by its RESULT.** A caught
  `fiber/abort` hands the injected payload straight back to its caller, whose
  `DecrefValueRegion` then fires on it alongside the separate release the caller
  already owes it as an *argument* — and the unwinding child runs no `Return` to
  fund the second. So the caught arm mints that reference
  (`handle_fiber_abort_signal`), and only that arm: the uncaught arm pushes `nil`
  and routes the payload through the signal, where no caller release targets it,
  and an in-body handler that catches the error consumes the payload inside the
  fiber and hands the caller a value of its own. Minting at the delivery instead
  would strand a region per abort on both of those paths
  (`tests/elle/region-fiber-abort-delivery-uaf.lisp` carries the over-free face
  and the two placement faces).
- **`Mixed`** — examined, and the native stores arguments *uncounted*
  (the property the arg clique exists to cover) — and/or returns a result
  that is neither always-fresh nor always-pass-through (a trait-dispatching
  primitive that may run a user closure). A positive declaration — "we read it; this is the honest
  worst case." A primitive that stores nothing but merely returns a
  non-fresh result is **`Opaque`** (above), not `Mixed` — the clique is
  keyed on the *store*, so a non-storing native must not carry it.
- **`Unknown`** — nobody has looked. The default for unexamined
  primitives, every plugin-supplied definition (the plugin ABI cannot
  carry a claim yet), and the standing classification of user-supplied
  functions and unknown callees in the solver. Treated exactly like
  `Mixed` operationally (full clique, no oracle check) — the distinction
  is epistemic: `Unknown` is the declaration work queue, while `Mixed` is
  settled and should not be revisited expecting a free upgrade. Over the
  canonical tables (`registration::ALL_TABLES`) that queue is empty, and a
  test holds it empty — a new primitive that omits `effect:` inherits the
  clique by silence, so the omission fails the build
  (`every_primitive_declares_an_examined_region_effect`).

**What the solver derives.** For an opaque (non-inlined) call the solver's
baseline assumes every heap argument may be stored into every other —
the *arg clique*: mutual may-store edges, a compile-time `IncrefRegion` of
each heap argument's region at the call site, balanced by the target
region's free-time cascade *only if the store actually happens*. A clique
incref against a store that never happens never balances — an over-keep
(never a mis-free) that leaks one region per uncounted edge per call.

**The clique is over pairs of ARGUMENTS, never over one argument's own
regions.** A single argument's value may have several source regions — the
arms of a branch, a pattern alias into a scrutinee — and those are
*alternatives* for the one value the call receives, never two values one
could store into the other. So a native reached with one heap argument
emits no edge however many regions that argument carries, and a store of
that argument into another one is covered by the edges to the *other*
argument's regions. Pairing a flattened region list instead strands one
region per call on a shape with no second argument at all
(`region-native-effect-clique-leak.lisp` § "One argument, two source
regions"); the declared-store path states the same rule as its `j == i`
skip (`record_store_edges`).

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
- `Stores { args }` (and `Sends { args }`, identically for edges): a directed
  may-store edge from each listed argument's regions to each *other* heap
  argument's regions (the possible in-argument targets). A store into the
  result needs no compile-time edge — the result object's alloc-time scan
  counts it (Rule 5, immutable contents) or the mutable-store funnel does. A
  store into an external structure must be runtime-counted by the native
  itself (`incref_for_escape`); it is invisible to the solver by nature and
  the declaration documents it. `Sends` records the same edges (shared
  `record_store_edges`) and additionally seeds the listed args as
  fiber-frontier crossings — the only behavioral difference.
- `Mixed` / `Unknown` (a registered **native** whose store behaviour is
  uncounted-or-unexamined): the full mutual clique. A native can
  reach value/ internals and store an argument *uncounted* — invisible to
  both the funnel seam and the solver — so the clique is its only cover.
- **User functions and other non-primitive callees** (the solver's `None`
  case — the callee is not in the primitive effects table): **no edges**.
  This is *not* `Unknown` in disguise. A user function is ordinary Elle code,
  and Elle code can store a value into a mutable container *only* through the
  mutable-store funnel (`push`/`put`/`add`/`%put` and friends), which is
  runtime-counted and statically complete (Rule 5) — the raw `Value`-bearing
  cells are reachable only inside value/. So every store a user fn performs on
  an argument is already counted at the store site (or by an edge in the
  callee's *own* compilation), and a caller-side clique incref is pure
  redundancy: emitting one would leak one region per *alloc-region* heap
  argument per call (a literal's static slot IS populated, so its
  `IncrefRegion` is real and never balances), while a call-result argument is a
  slot-based no-op (`region-userfn-clique-callresult-noleak.lisp`).
  Pinned by `region-userfn-clique-noleak.lisp`.

The result side is unchanged by declarations at runtime — the call-result
placeholder and value-gated `DecrefValueRegion` release (Rule 2) remain the
machinery for every heap-returning effect; `Immediate` calls contribute no
result regions to the walk (the solver's `call_returns_immediate` check,
keyed on this declaration).

**The dispatch pass-through retain, what it is for, and the declarations that
waive it.** `dispatch_native_call` funds the caller's release: a heap result
living outside the call's own minted region gets one owning reference (the
pass-through retain), so the caller's `DecrefValueRegion` balances against it
instead of freeing a region owned elsewhere.

The retain is for a **result**, so it is taken only when the native returns one —
`SIG_OK`. A native that returns a value as a **signal payload** hands it to the
signal machinery, which accounts for it on the path that payload actually takes:
a fiber carrier (`fiber/resume`/`fiber/abort`/`fiber/propagate` returning their
fiber ARGUMENT) is replaced by the child's outcome before any caller release
runs; a suspending payload rides `fiber.signal` under the `SuspendEscape` /
`EmitEscape` retain and is released on the resume path; an error or halt payload
is read through the signal, never through the caller's result slot, which the
handler stamps `nil`. There is no consumer for a retain on any of those, so
taking one strands a region per call — the emitted value of every `fiber/emit`
(`tests/elle/region-fiber-install-clique-leak.lisp`), or a
parked-then-discarded fiber's whole region graph (the `multi-resume` /
`yield-discard` oracle probes). This is the same exemption the declaration oracle
makes for a signal-carrying return, stated on the accounting side.

Beyond that, two `PrimitiveDef` flags declare "the body already supplied that
reference," and the dispatch then skips the retain — taking it anyway would hand
the caller two references against one release, one stranded region graph per
call:

- **`moves_out`** — the result is an element REMOVED from a container argument,
  and the body took the retain in place, necessarily before releasing the
  container's own reference (`arena::pop_with_decref`; the `raw-pop` oracle
  probe pins the double-count).
- **`result_minted`** — the result was produced by compiled code run on the
  driving VM (`import`'s module body, the `compile/*-module` test loaders'
  setup accumulator, each via `run_thunk_to_completion`), so it left that code
  through the return convention already carrying the caller's reference — a
  **thunk-run result**. The claim binds every normally-completing path: a
  declarant path that runs no thunk supplies the reference itself (`import`'s
  plugin paths take an explicit `EscapeSite::NativeCallResult` retain).
  Consumed at dispatch only — no solver site reads it. Pinned by the
  `import-result` probe in `tests/elle/oracle.lisp`.

A thunk-run value that is *embedded* in a fresh result rather than returned
bare needs no flag but still owes the mint's consumption: the fresh container's
alloc-time scan counts the embedding, so the boundary consumes the mint after
the container is built and the cascade frees the value with it
(`handle_arena_allocs`; the `allocs-result` oracle probe).

What the result side *does* derive is an **alias** fact for the ownership
forest. `Fresh`, `Stores` and `Sends` each claim a heap result in the call's
own minted region — the claim the declaration oracle below checks — and
`Immediate` claims no region at all, so none of the four can hand back a value
living inside an *argument*. Every other effect can (`PassThrough` by
definition, `Funnel`'s in-place container return, `Opaque`, `Delivers`, `Mixed`,
`Unknown`), and a non-primitive callee is under no claim whatsoever. For those
the walk records `result ⊒ each argument`
(`RegionInfo::opaque_result_aliases`), so a subtree whose member such a result
may name is bound by the root's drop or refuses to Shared — the result-side
analogue of the arg clique, and the reason `Fresh` is worth declaring even for
a primitive that stores nothing ([adopt.md](adopt.md) § "The lifetime
obligation the root carries").

`Funnel` is the one middle case: its result is arg0 in place or a fresh copy of
arg0 — the container either way, never an element interior to it — so it needs
no bound, only the reachability that a read out of that result is a read out of
arg0 (`RegionInfo::funnel_result_containers`). The container READS are excluded
from both relations: their result *is* the interior element, and the borrow
face's own edge records it against the tighter container. That exclusion is
keyed on the read set itself (`CallClassification::container_read_funnels` —
`get`/`first`/`rest`/`pop` and their `%`-op peers), not on the declared effect,
so it holds however each read is declared.

**Hard edges: how a may-store edge is emitted.** An edge's compile-time
incref is keyed by the *source* region. For a region minted by an alloc
opcode the static slot resolves at runtime and a slot-based `IncrefRegion`
is correct. For a **call-result placeholder** the slot is never populated
(only alloc opcodes record region mints in the activation map), so the
slot-based incref is a silent no-op — while the edge's balancing decref,
the store target's free-time cascade, is real. If the store happens, the
cascade steals a live reference: the call-result-arg clique UAF
(tests/elle/region-native-clique-callresult-uaf.lisp).

The fix is split by who recorded the edge:

- Edges recorded at a **native call site with a declared uncounted-store
  effect** (`Stores` / `Sends` / `Mixed` / `Unknown` — the callee is a known
  primitive) are **hard edges**: the store is real or must be presumed
  real. For a call-result source the lowerer increfs by *value* — load
  the argument from its binding slot and retain the runtime region the
  value actually lives in — so the cascade's decref is balanced.
- An **opaque user-fn call site** (callee effect `None` — not a registered
  primitive) records **no clique edges at all** (see the user-functions
  bullet above). Every store a user fn performs on an argument goes through
  the runtime-counted mutable-store funnel or is counted by an edge in the
  callee's own compilation, so no caller-side edge is needed; a slot-based
  edge would leak one region per alloc-region argument per call, and a
  value-based one would leak the call-result case too (the funnel already
  counts the store). One residual remains: a user fn that dispatches a storing
  native over a *call-result* argument (the higher-order `chan/send` shape) is
  a UAF candidate only when the dispatched native is not statically resolvable
  at the call site (`call_effect` returns `None`, so no hard edge is recorded).
  When the native *is* resolvable in the callee's own compilation it is a
  hard-edge site like any other `Stores`/`Sends`/`Mixed`/`Unknown` native call, and
  the value-based incref above covers the call-result source there.

**The declaration oracle.** A declaration is a soundness claim, so it is
checked, forever: in debug builds `dispatch_native_call` compares the
declared effect against `region_of(result)` after every native call that
completes normally — `Immediate` ⇒ the result has no region; `Fresh` /
`Stores` / `Sends` ⇒ a heap result lives in the call's own minted region;
`PassThrough` ⇒ a heap result lives anywhere *but* the call's own minted
region; `Funnel` / `Mixed` / `Unknown` / `Opaque` / `Delivers` ⇒ no check (an
`Opaque` or `Delivers` result may live anywhere — that is the point of both
variants). A violation panics deterministically, naming the primitive.
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
declare a statically-known return type (`PrimitiveDef::ret`), consumed by type
inference (the `type-of` dispatch prune) and by the ownership forest at two
points: a `Funnel` store's **container** classification (a
`MutableArray`/`MutableStruct` container retains the stored value's region, so
the walk recovers a containment edge; a `@string`/`@bytes` container copies
bytes and retains nothing), and the **fiber-member refusal** — the result region
of a call whose declared type is `Fiber` (`fiber/new`) is recorded in
`RegionInfo::fiber_result_regions` and is never adoptable by any region-rooted
cut (adopt.md § "The fiber member — refused at the class level"). A `RetType`
claim must hold on **every** normally-completing path: a nullable result
(`fiber/child`, which returns nil before any resume) declares `Unknown`, never
the heap type, or the prune would cut a live `nil` dispatch arm.
