(elle/epoch 8)
## Test: JIT with mutable captured parameter, no yield
## Does the JIT handle mutable-captured params WITHOUT yielding?

(defn test-mutable-param [@n]
  (let [inc (fn [] (assign n (+ n 1)))]
    (inc)
    n))

(def @i 0)
(while (< i 20)
  (let [result (test-mutable-param 0)]
    (when (not (= result 1))
      (eprintln "FAIL: expected 1, got" result)
      (sys/exit 1)))
  (assign i (+ i 1)))

(eprintln "PASS")
