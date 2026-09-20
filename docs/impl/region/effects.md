# Native region effects: declared, not guessed

<!-- audited: 2026-09-19 -->

How each primitive declares its region behavior, and what each `RegionEffect`
variant claims.

This is the native-call analogue of Rule 2's opaque-call exception and Rule 5's
escape list (see [rules.md](rules.md)), made explicit per callee instead of
assumed worst-case for all.

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
- **`Sends { args }`** — the listed arguments cross a **fiber boundary**:
  they are handed to another fiber (`chan/send`'s message rides the channel
  to the receiving fiber, by pointer under the single-threaded scheduler).
  The store is **seam-counted**: the send body increfs the message's region
  at runtime after a successful enqueue (`EscapeSite::ChanSend` in
  `prim_chan_send`), so the solver records **no edge** — exactly as for
  `Funnel` and `Delivers`. A compile-time edge cannot carry this reference:
  its incref is keyed on a region *pair* the solver must name, and at a real
  call site the channel is typically a module-level binding read as an
  upvalue, so no pair exists and no incref is emitted — the message then
  rides the buffer on the sender's own references and is freed before the
  receive (`tests/elle/region-chan-send-owned-param-uaf.lisp`). The buffer
  is **external** to the region system — no free-time cascade balances
  anything stored in it — so the seam retain IS the message's reference. The
  message is a genuinely-Shared (no-bounded-dominator) region, and its
  incoming count is maintained the way § class 7 of
  [ownership.md](ownership.md) prescribes: the send bumps it (the seam
  retain), and the **receive lowers it** — `chan/recv` / `chan/try-select` /
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
  fiber resume — the `subprocess` value `subprocess/exec` answers).
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
  `frame_held_regions` — the branch-arm release window among them. A
  declaration is therefore a claim about escape as much as about edges, and the
  strongest true one is what a read-only dispatcher owes
  (`tests/elle/region-sequence-read-effect.lisp`).

  **A native that re-enters the VM is `Opaque` on the store side.** `vm/query`
  selects an operation by a runtime string; `compile/run-on` dispatches a
  caller-supplied closure on a chosen tier; the `compile/*-module` loaders and
  `import` compile a module and run its top level as a thunk. None of that reaches
  the store side: each copies its arguments
  out (a Rust `String`, a cloned `Syntax`) and the Elle code it re-enters can
  store only through the runtime-counted funnel, exactly as an opaque user fn
  can. The *result* is what the re-entry makes unbounded, so the answer is again
  `Opaque` — and the obligation it carries is on the dispatch, not the primitive:
  an operation added behind one of these gateways that RETAINS an argument past
  the call invalidates the declaration and must move it back to `Mixed`
  (`tests/elle/region-query-clique-leak.lisp`). `import` is the declarant whose
  result side is furthest from its own region: the module value comes back through
  the thunk's return convention already carrying the caller's reference
  (`result_minted`, below), or — for a plugin already loaded — out of the plugin
  cache, minted by an earlier call. The specifier is resolved through a Rust
  `String` and never retained, so the store side is empty
  (`import_declares_opaque_no_hard_edge`, `import_does_not_seed_the_store_facet`;
  the soundness face is `tests/elle/region-fiber-child-effect-uaf.lisp`).

  **A fiber-graph read is `Opaque`.** `fiber/child` hands back the cached
  child-fiber `Value` its argument carries. The cache is written by the resume
  machinery (`with_child_fiber`), not by this call, so the read itself stores
  nothing; the value it returns lives in whatever region the child was minted in,
  which is neither the call's own nor its argument's. Unbounded result, no store —
  `Opaque`, and its argument is not a store-facet escape seed. What a `Mixed`
  declaration costs a read like this is that seed: a fiber named in one arm of a
  branch and read in another loses the branch-arm release window and strands per
  call (`tests/elle/region-fiber-child-effect.lisp`).

  **The child-chain WIRING is `Opaque` too — that write holds nothing.**
  `fiber/propagate` returns `SIG_PROPAGATE` carrying its fiber argument, and
  `handle_fiber_propagate_signal` writes that value into the propagating fiber's
  own `child`/`child_value` fields. The `Mixed` rule below asks whether a handler's
  write creates a **holder**, and this pair is not one. The free-time walk's Fiber
  arm enumerates the closure, its env and template, the terminal `signal`,
  `closure_value` and the seeded parameter baseline — never the child chain
  (`find_object_cross_refs`). A field no walk consults owes no reference, so
  nothing is under-counted by leaving it undeclared.

  `fiber/resume` settles the same question the same way. Its handler performs the
  identical two writes on every resume — `with_child_fiber` step 2 when it
  descends, `seed_child_inheritance` when it suspends instead — and its
  `Delivers { args: &[1] }` lists the resume value alone. The fiber argument has
  never been a store there, and the two wiring natives agree. What keeps the
  cache honest is not a reference but its consumer: `fiber/child` reads it only
  while the wiring call's own frame is live or parked, and absorbing the child's
  signal clears both fields (`VM::absorbs`'s callers).

  The cost of `Mixed` here is the branch-arm seed the read rule above describes,
  and `defer` is where it lands. `defer` reads its fiber with `fiber/status`, with
  `fiber/value` in one arm and with `fiber/propagate` in the other, so a store
  facet on the propagate argument leaves the branch's only release in the arm the
  success path never takes. Every evaluation then strands the fiber value and its
  body closure, and a loop whose body is wrapped in `defer` grows without bound
  (the `defer-while` and `defer-error` probes in `tests/elle/oracle.lisp`).
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
    the next step CONSUMES is handed straight back out of the slot by
    `do_fiber_resume_single` and pushed onto the resumed frame's stack, and it is
    the RESUMED frame that counts it: the suspending call's continuation mints the
    resume value it re-enters on — the parked native call's own result retain, or
    the `Emit`'s (`RegionInfo::unfunded_resume_values`,
    [park.md](park.md) § "A resume value crosses counted, or not at all"). A
    compile-time incref at the install would double-count the first against its
    single cascade decref and duplicate the second, which is exactly the arg-clique
    leak (`tests/elle/region-fiber-install-clique-leak.lisp`).
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

  The distinction from `Sends` is *who balances the seam's reference*: both
  seams count their own store at runtime and record no solver edge. `chan/send`
  leaves its message in a channel buffer external to the region system, which
  nothing cascades, so the send-site retain IS the message's reference and
  `chan/recv` lowers it. A fiber's signal slot is not external — it is a
  scanned field of a region-managed fiber object — so an outliving install is
  balanced by the fiber's free-time signal scan, and a consumed one by the
  resumed frame's own accounting.

  **An injected error payload arrives unfunded, so the injection mints its
  delivery.** Every other install of a terminal payload into a signal slot funds
  itself: a raise mints (the `EmitEscape` retain, or a fresh error struct's birth
  reference), and a re-park mints (`PropagateEscape`). `fiber/abort` and
  `fiber/refuse` install a payload the CALLER owns, and the caller's one
  reference answers the caller's *argument* release alone. Exactly one further
  release fires on that payload as a RESULT, and which one depends on where the
  injected error stops:

  - the fiber's mask catches it, and the abort's caller releases the payload as
    the call's result;
  - a `protect` or `try` inside the fiber catches it, and that handler's release
    of its own resume result consumes it;
  - the error escapes the fiber, and the resume result of whichever ancestor
    absorbs it consumes it;
  - the unwinding replays a parked `defer`/`protect` frame, and that frame's
    suspending call runs its compiler-emitted result release on it.

  One reference, one consumer, four routes. So `inject_error_at_suspension` mints
  it once at the seam all four leave through, and no route has to recognize
  itself. Each route's own delivery mint is therefore absent: `do_fiber_abort`
  hands the replayed frame the injection's reference rather than taking a
  `ReturnValue` retain of its own, and the caught arm hands the caller the same
  one.

  The injection also RECORDS the mint, on both fibers whose frames the payload
  travels through — the aborted fiber (`do_fiber_abort`) and, where the error
  escapes, the aborting one (`VM::park_propagating_abort`). A frame holding the
  payload then funds no delivery, so the abandoned-frame walk and the parked
  frame's discharge stop exempting the payload's region, exactly as they do for
  an emit raise ([mechanism.md](mechanism.md) § "An abandoned frame runs the
  releases it still owes"). A literal materialized straight into the
  `fiber/abort` argument lives in a frame slot and nowhere else, so without the
  record its release stays owed forever (the `abort-discard` probe in
  `tests/elle/oracle.lisp`).

  `tests/elle/region-fiber-abort-delivery-uaf.lisp` carries a face per route: the
  under-mint faults there under `--trace=guardfree`, and the over-mint shows as
  region growth, since a spare reference never faults.
- **`Mixed`** — examined, and the native stores arguments *uncounted*
  (the property the arg clique exists to cover) — and/or returns a result
  that is neither always-fresh nor always-pass-through (a trait-dispatching
  primitive that may run a user closure). A positive declaration — "we read it; this is the honest
  worst case." A primitive that stores nothing but merely returns a
  non-fresh result is **`Opaque`** (above), not `Mixed` — the clique is
  keyed on the *store*, so a non-storing native must not carry it.

  **A signal a handler stores for is a store.** A primitive whose work is done by
  the VM's handler for its signal is judged on what that handler does with the
  argument. `git` returns `SIG_QUERY` carrying the closure it is asked to compile,
  and the handler caches the SPIR-V on that closure's template — a retention that
  outlives the call, shared by every closure over that template, and recorded by no
  seam. So the store is real and uncounted, which is `Mixed`'s property, and the
  argument stays a store-facet escape seed.

  The rule asks for a **holder**, not merely for a write. A handler that writes an
  argument into a field the free-time walk never enumerates creates no second
  holder of the region and can under-count nothing, so the strongest true claim is
  the one that says so — `Opaque`, the child-chain wiring's answer (§ `Opaque`,
  "The child-chain WIRING is `Opaque` too"). Both halves are pinned solver-side by
  `fiber_graph_natives_declare_opaque_and_git_keeps_the_hard_edge`.
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


What the solver derives from these declarations — the argument clique, the
hard edges, and the oracle that checks the claim on every debug run — is in
[what a declaration buys](clique.md).
