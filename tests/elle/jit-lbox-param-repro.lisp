(elle/epoch 9)
## Test: JIT yield with mutable captured parameter
## A parameter captured by a nested closure AND mutated needs LBox wrapping.

(defn test-mutable-param [@n]
  (let [inc (fn [] (assign n (+ n 1)))]
    (println "before:" n)
    (inc)
    (println "after:" n)
    n))

(def @i 0)
(while (< i 20)
  (let [result (test-mutable-param 0)]
    (when (not (= result 1))
      (eprintln "FAIL: expected 1, got" result)
      (sys/exit 1)))
  (assign i (+ i 1)))

(eprintln "PASS")
