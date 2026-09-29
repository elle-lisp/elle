(elle/epoch 12)
## ── Fuel: apply and fold preemption bugs
##
## Reproduces two runtime bugs where fuel preemption corrupts state:
## 1. fold accumulator reset — after fuel preemption, accumulator silently
##    resets to its initial value instead of preserving progress.
## 2. apply splice corruption — after fuel preemption, apply's splice
##    sees nil instead of the argument list.
##
## Both work outside process:start (no fuel preemption). Root cause is in
## the Rust runtime — apply's splice and fold's tail-call don't preserve
## state correctly across fuel-exhaustion yields.
##
## Fuel values are generous because fold and apply are implemented in Elle
## (not Rust primitives), consuming more instructions per iteration.
##
## Fibers use |:fuel :error| mask so that runtime errors from the bugs
## are caught by the fiber rather than propagating to the test runner.

(def @failures 0)

(defn check [name ok]
  (if ok
    (println "  PASS:" name)
    (do
      (println "  FAIL:" name)
      (assign failures (+ failures 1)))))

# ── Scenario 1: fold accumulator loss ──────────────────────────────────
#
# fold over (range 100) summing with fuel=1000 (insufficient to complete
# without preemption). After refuel+resume, expect value 100; the bug
# corrupts the accumulator causing a type error or wrong value.

(println "Scenario 1: fold accumulator loss")
(let [f (fiber/new (fn [] (fold (fn [acc _] (+ acc 1)) 0 (range 100)))
                   |:fuel :error|)]
  (fiber/set-fuel f 1000)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel" (= (fiber/status f) :dead))
  (check "accumulator preserved (expected 100)" (= (fiber/value f) 100)))

# ── Scenario 2: fold with concat ──────────────────────────────────────
#
# fold building a bytes buffer by appending one byte per iteration over
# (range 200) with fuel=500. After refuel+resume, expect 200-byte result;
# the bug corrupts the accumulator.

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

# ── Scenario 3: apply splice corruption ───────────────────────────────
#
# (apply + (range 100)) with fuel=500. After refuel+resume, expect 4950;
# the bug throws "splice: expected array, tuple, or list, got nil".

(println "Scenario 3: apply splice corruption")
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
# (apply concat ...) over 100 two-byte arrays with fuel=500. After
# refuel+resume, expect 200 bytes.

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

# ── Scenario 5: Baseline — fold without preemption ────────────────────
#
# Same fold as test 1 but with fuel=10000000 (enough to complete without
# preemption). Proves fold works when fuel is sufficient.

(println "Scenario 5: baseline fold without preemption")
(let [f (fiber/new (fn [] (fold (fn [acc _] (+ acc 1)) 0 (range 100)))
                   |:fuel :error|)]
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes with sufficient fuel" (= (fiber/status f) :dead))
  (check "correct value without preemption (expected 100)"
         (= (fiber/value f) 100)))

# ── Summary ───────────────────────────────────────────────────────────

(println)
(if (= failures 0)
  (println "All checks passed.")
  (do
    (println failures "check(s) failed.")
    (assert false "fuel-apply-fold: regression tests failed")))
