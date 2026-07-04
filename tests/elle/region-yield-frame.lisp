#!/usr/bin/env elle
(elle/epoch 12)

# Regression tests for the per-execution region model across fibers
# (docs/regions/semantics.md — every value its own region).
#
#   1. Cross-yield region-frame preservation. A fiber allocates a
#      region-managed value, yields (suspending its activation), and on
#      resume the per-activation static->physical region remap must survive
#      so post-resume allocs/decrefs resolve in the same frame, not in some
#      caller's frame whose slots could free a physical region still in use.
#      Values produced across the yield boundary must stay correct, and a
#      captured top-level struct must not be disturbed by the driver's churn.
#
#   2. Fiber closure cross-region retention. A fiber holds its closure; the
#      closure's captured env lives in the *spawning* activation's region.
#      The fiber must establish a cross-region edge to that region — see
#      `collect_value_refs` for `HeapObject::Fiber` — or the spawning
#      activation frees the closure's env before a scheduler-parked fiber
#      first resumes, and its captures then read back as garbage.
#      Counterfactual: before the fix, the captured struct below read as an
#      integer in the spawned fiber.

(def @failures @[])
(defn check [name ok msg]
  (unless ok (push failures (string name ": " msg))))

# ── 1. Cross-yield region-frame preservation (pure fiber/resume) ────────
(def @tbl @{:k 1})
(defn gen [n]
  (def @i 0)
  (while (%lt i n)
    (let [x (%pair i i)
          junk {:a i :b [i i i]}]
      (yield (%add (first x) (get junk :a))))
    (assign i (%add i 1))))

(let [g (fiber/new (fn [] (gen 200)) |:yield|)]
  (def @j 0)
  (def @bad 0)
  (while (%lt j 200)
    (let [v (fiber/resume g)]
      (unless (= v (%mul 2 j)) (assign bad (%add bad 1))))  # churn the driver's own regions between resumes
    (let [m (%pair j j)
          s {:x j}]
      (%add (first m) (get s :x)))
    (unless (struct? tbl) (assign bad (%add bad 1)))
    (assign j (%add j 1)))
  (check "cross-yield-values" (= bad 0) (string "bad=" bad))
  (check "cross-yield-capture" (= (get tbl :k) 1) (string "tbl=" tbl)))

# ── 2. Fiber closure cross-region retention (scheduler-parked) ──────────
# Top-level heap captures must survive being parked in the scheduler queue
# until first resume — for both non-yielding and yielding spawned bodies.
(def @cap-struct @{:k 42})
(def cap-int 7)
(def cap-arr @[10 20 30])

(let [r (ev/spawn (fn [] [(get cap-struct :k) cap-int (get cap-arr 0)]))]
  (check "spawn-capture-nonyield" (= (ev/join r) [42 7 10]) "non-yielding"))

(let [r (ev/spawn (fn []
                    (ev/sleep 0.001)
                    [(get cap-struct :k) (get cap-arr 2)]))]
  (check "spawn-capture-yield" (= (ev/join r) [42 30]) "yielding"))

(if (empty? failures)
  (println "region-yield-frame: PASS")
  (begin
    (println "region-yield-frame: FAIL")
    (each f in (freeze failures)
      (println "  " f))
    (exit 1)))
