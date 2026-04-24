(elle/epoch 9)
# Test: multiple yielding calls in the same function
# Regression test for the WASM drive_resume_chain stale-frame bug.
# When a resumed WASM closure yields again, old outer frames must
# be evicted so the new yield chain is consumed in the right order.

# Two println calls (each yields for I/O in WASM backend)
(defn two-yields []
  (println "a")
  (println "b")
  42)

(let [result (two-yields)]
  (assert (= result 42) "two-yields: expected 42"))

# Three println calls
(defn three-yields []
  (println "1")
  (println "2")
  (println "3")
  99)

(let [result (three-yields)]
  (assert (= result 99) "three-yields: expected 99"))

# Yielding call + mutation via LBox + yielding call
(defn yield-mutate-yield [@n]
  (let [inc (fn [] (assign n (+ n 1)))]
    (println "before:" n)
    (inc)
    (println "after:" n)
    n))

(let [result (yield-mutate-yield 0)]
  (assert (= result 1) "yield-mutate-yield: expected 1"))

(eprintln "PASS: wasm-multi-yield")
