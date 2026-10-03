(elle/epoch 12)
# audited: 2026-09-29
# A def inside a lambda, captured by a sibling closure, releases its init through the env, not a stack slot.
# docs/impl/region/cells.md
#
# Such a binding is env-celled: `allocate_slot_routed` mints its slot from the
# env address space (the index `StoreCapture`/`LoadCapture` address), while a
# plain local's comes from the stack space. Both are `u16`, and both reach
# `region_to_slot`, whose entries `emit_decrefs_for` turns into a value-route
# release. `ValueSlot` (src/lir/lower/naming.rs) carries the space with the
# index.
#
# The counter-factual: an env index read as a stack slot names an unrelated
# local — here the sibling closure itself — so the init's release frees the
# closure, and the closure's own release then reads freed pages.
#
# Two halves are pinned separately, because each can go wrong without the
# other. A release routed to the env through the wrong space frees a live
# local, and a missing env-routed release strands the init's region on every
# execution. This file owns the first: the def and its holder both survive.
# The second is the `env-cell-def-capture` row of the oracle
# (tests/impl/probe/concurrent.lisp), read against the `env-cell-let-twin`
# control: a `let`-bound immutable local is captured by value and never gets a
# cell, so the gap between the two isolates the env route.
#
# tests/lang/defer-def.lisp drives the same shape through `defer`, which runs
# its body in a fiber: a `def` in a defer scope is a `def` inside a lambda,
# and a nested `defer` reading it is the sibling closure.

(defn increment [x]
  (+ x 1))

# ── The def and its holder both survive ──
#
# The `def`'s initializer must be a CALL — a constant is folded and allocates
# nothing to mis-release. The sibling closure must capture a SECOND binding
# beside the def; with one capture the env index does not reach a live local.

(defn reads-through-the-closure []
  (let [root "/x"]
    ((fn []
       (def joined-path (path/join root "a"))
       (let [reader (fn [] (list (string? joined-path) (increment 1)))]
         (reader))))))

(assert (= (list true 2) (reads-through-the-closure))
        "the def and the defn both read back through the sibling closure")

# The closure need not be CALLED: the release fires at the binding's decref
# point, so merely constructing the capture was enough to free the wrong local.
# Nothing reads the def afterwards — the pin is that this returns at all.
(defn builds-an-uncalled-closure []
  (let [root "/x"]
    ((fn []
       (def unread-path (path/join root "b"))
       (let [_reader (fn [] (list (string? unread-path) (increment 1)))]
         :built)))))

(assert (= :built (builds-an-uncalled-closure))
        "constructing an uncalled sibling closure frees nothing")
