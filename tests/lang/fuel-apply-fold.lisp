(elle/epoch 18)
# audited: 2026-11-20
# A fold and an apply keep their state across a fuel pause and a refuel.
# docs/runtime.md
#
# The trap: each budget has to run out inside the body, so each scenario's first
# check reads that the fiber paused. A call and a backward jump cost one unit
# each, so a stdlib that makes fewer calls shrinks what a body costs. Each body
# costs several times its budget, so its fold or apply is still running when
# the budget runs out.
#
# The fibers mask |:fuel :error|, so a runtime error the resume raises becomes
# the fiber's value and fails a check, and the runner keeps going.

(def @failures 0)

(defn check [name ok]
  (if ok
    (println "  PASS:" name)
    (do
      (println "  FAIL:" name)
      (assign failures (+ failures 1)))))

# ── Scenario 1: fold accumulator across a pause ────────────────────────
#
# A fold over (range 1000) that counts its elements, with fuel=1000. After the
# refuel the count is 1000. A lost accumulator answers a wrong count or a type
# error.

(println "Scenario 1: fold accumulator across a pause")
(let [f (fiber/new (fn [] (fold (fn [acc _] (+ acc 1)) 0 (range 1000)))
                   |:fuel :error|)]
  (fiber/set-fuel f 1000)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel" (= (fiber/status f) :dead))
  (check "accumulator preserved (expected 1000)" (= (fiber/value f) 1000)))

# ── Scenario 2: fold with concat ──────────────────────────────────────
#
# A fold that appends one byte per element of (range 200), with fuel=500. After
# the refuel the buffer holds 200 bytes.

(println "Scenario 2: fold with concat")
(let [f (fiber/new (fn []
                     (fold (fn [acc _] (concat acc (bytes 65))) (bytes)
                           (range 200))) |:fuel :error|)]
  (fiber/set-fuel f 500)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel" (= (fiber/status f) :dead))
  (check "200 bytes preserved across preemption"
         (and (= (fiber/status f) :dead) (= (length (fiber/value f)) 200))))

# ── Scenario 3: apply splice across a pause ───────────────────────────
#
# (apply + (range 100)) with fuel=500. After the refuel the sum is 4950. A lost
# splice argument throws "splice: expected array, tuple, or list, got nil".

(println "Scenario 3: apply splice across a pause")
(let [f (fiber/new (fn [] (apply + (range 100))) |:fuel :error|)]
  (fiber/set-fuel f 500)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel (no splice nil error)"
         (= (fiber/status f) :dead))
  (check "correct sum (expected 4950)" (= (fiber/value f) 4950)))

# ── Scenario 4: apply concat with many chunks ────────────────────────
#
# (apply concat ...) over 100 two-byte arrays with fuel=500. After the refuel
# the result holds 200 bytes.

(println "Scenario 4: apply concat with many chunks")
(let [f (fiber/new (fn []
                     (apply concat (map (fn [_] (bytes 65 66)) (range 100))))
                   |:fuel :error|)]
  (fiber/set-fuel f 500)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel" (= (fiber/status f) :dead))
  (check "200 bytes from 100 two-byte chunks"
         (and (= (fiber/status f) :dead) (= (length (fiber/value f)) 200))))

# ── Scenario 5: the fold of scenario 1, with no pause ─────────────────
#
# The same fold with fuel=10000000, which it never exhausts. The fold answers
# 1000 when nothing pauses it.

(println "Scenario 5: baseline fold without preemption")
(let [f (fiber/new (fn [] (fold (fn [acc _] (+ acc 1)) 0 (range 1000)))
                   |:fuel :error|)]
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes with sufficient fuel" (= (fiber/status f) :dead))
  (check "correct value without preemption (expected 1000)"
         (= (fiber/value f) 1000)))

# ── Summary ───────────────────────────────────────────────────────────

(println)
(if (= failures 0)
  (println "All checks passed.")
  (do
    (println failures "check(s) failed.")
    (assert false "fuel-apply-fold: regression tests failed")))
