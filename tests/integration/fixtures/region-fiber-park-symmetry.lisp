(elle/epoch 12)
# tests/integration/fixtures/region-fiber-park-symmetry.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression in either
# direction is loud: an OVER-FREE faults under --trace=guardfree (and the debug
# generation stamp panics at the stale deref), and `make smoke` globs
# tests/elle/*.lisp into one shared process where an abort takes the whole
# harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_fiber_park_symmetry_uaf`).
#
# WHAT IT PINS — park/unpark symmetry for fiber suspension
# (docs/impl/region/owner.md § "Park/unpark symmetry"):
#
#   1. The SIG_RESUME carrier is never retained at dispatch: a fiber that parks
#      (yield) and is then dropped — or drained across several yields — frees its
#      region (fiber + body closure + template) at its last reference. The
#      counterfactual is the dangling carrier pass-through retain: one per
#      suspending resume, never released, pinning every parked-then-discarded
#      fiber's region forever.
#   2. A suspending native TAIL call parks its post-`TailCall` continuation, so
#      the replay runs the compiler's owned-arg releases — the nested
#      `(fiber/resume g)`-in-tail-position shape frees the inner fiber.
#   3. A parked non-terminal signal's escape retain (EmitEscape / SuspendEscape)
#      is released when the fiber is killed or freed parked — the yielded struct
#      and the capability-denial payload reclaim.
#   4. A tail call to a literal-lambda callee defers the closure's region release
#      to the activation's completion (the `protect`-body shape).
#   5. A parked TERMINAL signal DISPLACED by a restart resume is released and
#      its recorded content edge un-recorded. An `:error` fiber is resumable
#      (the restarts system); the resume installs the resume value over the
#      parked error result, so the free-time signal scan the park-retain +
#      recorded edge were counting on never sees it. Without the displacement
#      release, the fiber's free detonates the debug equivalence oracle
#      (recorded frontier carries the dead signal edge the content scan
#      lacks), and a fiber that errors again after the restart parks a SECOND
#      edge — the free cascade then over-releases the payload region.
#
# TWO FAILURE MODES, one fixture:
#   - UNDER-release (the pre-fix leaks) — region-count grows per op; the asserts
#     below catch each face.
#   - OVER-release (a mis-fix releasing live parked state — e.g. a blanket
#     release of a parked frame's activation-map regions, whose stale entries
#     can name freed-and-recycled ids) — a stale-region deref once ids recycle;
#     the guardfree trace / generation stamp faults. State-dependent, so churn
#     runs long enough for ids to recycle.

# 1+3: a fiber parked at a yield (heap value) and dropped; a drained multi-yield
# fiber; a parked fiber hard-cancelled.
(defn churn-parks [n]
  (def @i 0)
  (while (< i n)
    (let [f (fiber/new (fn []
                         (yield {:x i})
                         99) |:yield|)]
      (fiber/resume f))
    (let [g (fiber/new (fn []
                         (yield 1)
                         (yield 2)
                         3) |:yield|)]
      (fiber/resume g)
      (fiber/resume g)
      (fiber/resume g))
    (let [h (fiber/new (fn []
                         (yield i)
                         9) |:yield|)]
      (fiber/resume h)
      (fiber/cancel h :dead)
      (fiber/status h))
    (assign i (+ i 1))))

# 2: the nested tail resume — the inner fiber is the outer body's tail arg.
(defn churn-nested [n]
  (def @i 0)
  (while (< i n)
    (let [f (fiber/new (fn []
                         (let [g (fiber/new (fn [] i) 1)]
                           (fiber/resume g))) 1)]
      (fiber/resume f))
    (assign i (+ i 1))))

# 3 (denial face) + the abort unwinding faces. The non-tail abort variant is the
# historical over-free reproducer: the aborted fiber re-parks for restarts while
# the root's continuation still reads values around it, so any discharge that
# releases live parked state faults here once ids recycle.
(defn churn-discards [n]
  (def @i 0)
  (while (< i n)
    (let [d (fiber/new (fn [] (println "blocked")) |:error :io| :deny |:io|)]
      (fiber/resume d)
      (get (fiber/value d) :error))
    # A denial the PARENT does not mask, caught by the grandparent: the
    # intermediate fiber's caught terminal signal is a propagated payload it
    # never held, parked with the same retain + recorded edge the child's own
    # exit takes — else its free-time signal scan drifts from the recorded
    # table and the cascade over-releases the payload.
    (let [outer (fiber/new (fn []
                             (let [inner (fiber/new (fn [] (length "x")) 0
                                   :deny |:error|)]
                               (fiber/resume inner))) |:error|)]
      (fiber/resume outer)
      (fiber/status outer))
    (let [a (fiber/new (fn []
                         (yield i)
                         9) |:yield|)]
      (fiber/resume a)
      (protect (fiber/abort a "boom")))
    (let [b (fiber/new (fn []
                         (yield i)
                         9) |:yield|)]
      (fiber/resume b)
      (protect (begin
                 (fiber/abort b "boom")
                 5)))
    (assign i (+ i 1))))

# 5: the restart resume — an :error fiber's parked terminal error struct is
# displaced by resuming it again (once, and once more after it errors again,
# so the multiplicity face is covered), then the fiber is discarded.
(defn churn-restarts [n]
  (def @i 0)
  (while (< i n)
    (let [f (fiber/new (fn [] (error {:x i})) |:error|)]
      (protect (fiber/resume f))
      (protect (fiber/resume f 42))
      (protect (fiber/resume f 43)))
    (assign i (+ i 1))))

# 4: the literal-lambda tail callee, bare and under protect.
(defn churn-protect [n]
  (def @i 0)
  (while (< i n)
    ((fn [] ((fn [] i))))
    (let [[ok v] (protect ((fn [] i)))]
      v)
    (assign i (+ i 1))))

# Prime: churn region ids so any freed page below is recycled onto a live region
# (an over-free is state-dependent and only faults after recycling).
(churn-parks 300)
(churn-nested 300)
(churn-discards 200)
(churn-restarts 200)
(churn-protect 300)

# Measure steady-state region growth per face. The discard faces carry a named
# bounded residual (the dead continuation's pending value releases —
# docs/impl/region/owner.md § "The bounded residual"), so their budget admits it;
# the closed faces must be flat.
(def p0 (arena/region-count))
(churn-parks 500)
(def p1 (arena/region-count))
(assert (< (- p1 p0) 50)
        (string "parked/drained/cancelled fibers leaked " (- p1 p0)
                " regions over 500 ops"))

(def n0 (arena/region-count))
(churn-nested 500)
(def n1 (arena/region-count))
(assert (< (- n1 n0) 50)
        (string "nested tail resume leaked " (- n1 n0) " regions over 500 ops"))

(def c0 (arena/region-count))
(churn-protect 500)
(def c1 (arena/region-count))
(assert (< (- c1 c0) 50)
        (string "literal-lambda tail callee leaked " (- c1 c0)
                " regions over 500 ops"))

(def s0 (arena/region-count))
(churn-restarts 500)
(def s1 (arena/region-count))
(assert (< (- s1 s0) 5500)
        (string "restarted :error fibers leaked " (- s1 s0)
                " regions over 500 ops — the displaced-terminal-signal "
                "release regressed"))

# The discard faces: today each op strands a bounded set of dead-continuation
# regions (denied ≈3, abort ≈5, the grandparent-caught denial ≈2 more). Pin the
# CEILING so the residual can only shrink: 500 ops × ~10 regions = 5000;
# anything past 5500 means a closed mechanism regressed.
(def d0 (arena/region-count))
(churn-discards 500)
(def d1 (arena/region-count))
(assert (< (- d1 d0) 5500)
        (string "discarded :error/denied fibers leaked " (- d1 d0)
                " regions over 500 ops — a closed park-symmetry mechanism "
                "regressed"))

(println "region-fiber-park-symmetry: ok")
