(elle/epoch 12)
# A `def` inside a lambda, captured by a sibling closure, releases its init
# through the ENV, not through the stack slot of the same number.
#
# Such a binding is env-celled: `allocate_slot_routed` mints its slot from the
# env address space (the index `StoreCapture`/`LoadCapture` address), while a
# plain local's comes from the stack space. Both are `u16`, and both reach
# `region_to_slot`, whose entries `emit_decrefs_for` turns into a value-route
# release. Read as a stack slot, an env index names an unrelated local — here
# the sibling closure itself — so the init's release freed the closure, and the
# closure's own release then read freed pages. `ValueSlot` carries the space
# with the index so that is unrepresentable.
#
# Two halves, pinned separately, because each fix direction alone breaks the
# other: routing the release to the env without correcting the space brings the
# free back, and dropping the mis-routed release without adding an env-routed
# one strands the init's region on every execution.
#
#   1. the def and its holder both survive (the free);
#   2. repeating the shape does not grow the live region count (the leak).
#
# `tests/elle/defer-def.lisp` is the same defect through `defer`, which runs its
# body in a fiber — so a `def` in a defer scope is a `def` inside a lambda, and
# a nested `defer` reading it is the sibling closure.

(defn increment [x]
  (+ x 1))

# ── 1. The def and its holder both survive ──
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

# ── 2. Nothing is stranded ──
#
# The init's reference has exactly one release. Without it the region survives
# every execution, so the count climbs by one per cycle rather than settling.

(defn growth [thunk]
  (var n 0)
  (while (< n 50)
    (thunk)
    (assign n (+ n 1)))
  (let [base (arena/region-count)]
    (assign n 0)
    (while (< n 200)
      (thunk)
      (assign n (+ n 1)))
    (- (arena/region-count) base)))

(assert (= 0 (growth reads-through-the-closure))
        "a def captured by a sibling closure strands no region per execution")

# The `let` twin is the discriminator for the leak half only. It is NOT a
# discriminator for the free half: a `let`-bound immutable local is captured by
# value and never gets a cell at all, so it never had an env index to confuse.
# `def` is always mutable, hence always celled, which is why only `def` reaches
# the defect.
(defn let-twin []
  (let [root "/x"]
    ((fn []
       (let [let-path (path/join root "a")]
         (let [reader (fn [] (list (string? let-path) (increment 1)))]
           (reader)))))))

(assert (= 0 (growth let-twin)) "the let twin strands nothing either")
