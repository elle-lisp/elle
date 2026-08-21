(elle/epoch 12)
# tests/integration/fixtures/region-fiber-exhume-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because the failure mode is an
# abort: a stale-region deref (the debug generation stamp) or a guardfree
# SIGSEGV, and `make smoke` globs tests/elle/*.lisp into one shared process.
# Exercised by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_fiber_exhume_uaf`).
#
# WHAT IT PINS — the fiber-member ownership refusal
# (docs/impl/region/adopt.md § "The fiber member — refused at the class level"):
# a fiber's region is never a member of a region-rooted Owned subtree, so a
# fiber handed back out of runtime graph state (`fiber/child`) rides an
# ordinary COUNTED pass-through retain that genuinely pins it.
#
# The shape: `inner` is sole-captured by `outer`'s body closure — exactly the
# capture the subtree walk would otherwise admit ({closure ⊇ inner}, externally
# unique). `outer` runs, the runtime wires `inner` into `outer`'s child chain,
# and `(fiber/child outer)` reads it back out — a reference created by the
# scheduler, invisible to every compile-time obligation. If `inner`'s region
# were adopted, the read's retain would land on a frozen RC and the release of
# `outer` (its last use) would subtree-drop `inner` under the returned borrow —
# a stale-region deref at the next use of the exhumed fiber.

# Face 1: the propagate child-chain read, error face.
(begin
  (let [inner (fiber/new (fn [] (emit :error "err")) 1)]
    (let [outer (fiber/new (fn []
                             (fiber/resume inner)
                             (fiber/propagate inner)) 1)]
      (fiber/resume outer)
      (assert (= (fiber? (fiber/child outer)) true)
              "exhume error face: child-chain read is a live fiber"))))

# Face 2: the propagate child-chain read, yield face.
(begin
  (let [inner (fiber/new (fn [] (yield 99)) 2)]
    (let [outer (fiber/new (fn []
                             (fiber/resume inner)
                             (fiber/propagate inner)) 2)]
      (fiber/resume outer)
      (assert (= (fiber? (fiber/child outer)) true)
              "exhume yield face: child-chain read is a live fiber"))))

# Face 3: the exhumed child OUTLIVES its outer fiber's whole capture family.
# The helper returns the child; `outer`, its body closure, and every capture die
# at the helper's exit, so the caller's read runs against a fiber whose only
# liveness is the read's own counted retain.
(defn exhume []
  (let [inner (fiber/new (fn []
                           (yield 7)
                           9) 2)]
    (let [outer (fiber/new (fn []
                             (fiber/resume inner)
                             (fiber/propagate inner)) 2)]
      (fiber/resume outer)
      (fiber/child outer))))
(let [c (exhume)]
  (assert (fiber? c) "exhumed child survives its outer fiber's release")
  (assert (= (fiber/resume c) 9) "exhumed child is still resumable"))

# Face 4: the refusal must not trade the UAF for growth beyond the F2
# dead-continuation residual. Each op discards `outer` parked at the
# propagate: its body loads the captured `inner` as the propagate's tail arg
# with the borrowed-tail-arg retain (`arg_leaf_is_borrowed`), whose consuming
# release sits in the post-`TailCall` continuation — parked by the suspend,
# runnable only by a restart replay that never comes (docs/impl/region/owner.md
# § "The bounded residual: a dead continuation's pending value releases"). So
# the discard strands inner's fiber region + its body-closure region: 2/op,
# bounded per discarded fiber. Pin the CEILING so the residual can only
# shrink: 500 ops × 2 = 1000; anything past 1100 means the fiber-member
# refusal itself regressed into a real leak. Prime id churn first (an
# over-free is state-dependent and only faults after recycling).
(defn churn-exhume [n]
  (def @i 0)
  (while (< i n)
    (let [inner (fiber/new (fn [] (yield i)) 2)]
      (let [outer (fiber/new (fn []
                               (fiber/resume inner)
                               (fiber/propagate inner)) 2)]
        (fiber/resume outer)
        (fiber? (fiber/child outer))))
    (assign i (+ i 1))))

(churn-exhume 300)
(def g0 (arena/region-count))
(churn-exhume 500)
(def g1 (arena/region-count))
(assert (< (- g1 g0) 1100)
        (string "exhumed-child churn leaked " (- g1 g0)
                " regions over 500 ops — past the F2 dead-continuation "
                "residual budget (2/op), so the fiber-member refusal "
                "regressed into a real leak"))

(println "region-fiber-exhume: ok")
