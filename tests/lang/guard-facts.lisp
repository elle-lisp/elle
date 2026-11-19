(elle/epoch 15)
# audited: 2026-11-18
# A type test answers for the value the binding holds when the test runs: after a loop turn, a call, a yield, or a closure's later call.
#
# The counter-factual: a compiler that carried an earlier test's answer to a
# later one, over a write it could not see in walk order, deleted the later
# test, and each program below computed a plausible wrong value.

# A loop's next turn reads what its last turn wrote.
(assert (= (let [@found nil]
             (while (nil? found)
               (each m in [1 2 5]
                 (when (nil? found) (assign found m))))
             found) 1) "the inner loop stops writing once found holds 1")

(assert (= (let [@x nil
                 @n 0
                 @hits 0]
             (while (nil? x)
               (while (< n 3)
                 (when (nil? x) (assign hits (+ hits 1)))
                 (assign x 1)
                 (assign n (+ n 1))))
             hits) 1) "the second turn of the inner while sees x written")

(assert (= (let [@x 0
                 @hits 0]
             (while (int? x)
               (each i in [1 2 3]
                 (when (int? x) (assign hits (+ hits 1)))
                 (assign x "s")))
             hits) 1) "the second turn sees the string")

# A call runs the closure that writes the binding.
(assert (= (let [@x nil]
             (def set-x (fn [] (assign x 1)))
             (match (type-of x)
               :nil
                 (begin
                   (set-x)
                   (match (type-of x)
                     :nil :nil
                     _ :int))
               _ :other)) :int) "the call wrote x after the arm tested it")

# The owner writes the binding while the fiber is parked at a yield.
(assert (= (let [@x nil]
             (def f
               (fiber/new (fn []
                            (match (type-of x)
                              :nil
                                (begin
                                  (yield 1)
                                  (match (type-of x)
                                    :nil :nil
                                    _ :int))
                              _ :other)) |:yield|))
             (fiber/resume f)
             (assign x 1)
             (fiber/resume f)) :int)
        "the parent wrote x while the fiber was parked")

# A closure's body runs after the write that ends the guard's value.
(assert (= (let [@x nil
                 @hits 0]
             (when (nil? x)
               (def count-nil (fn [] (when (nil? x) (assign hits (+ hits 1)))))
               (count-nil)
               (assign x 1)
               (count-nil))
             hits) 1) "the second call runs after the write")

(let [@x nil
      @probes @[]]
  (each i in [1 2]
    (when (nil? x)
      (push probes (fn [] (nil? x)))))
  (assert ((get probes 0)) "the probe runs before the write")
  (assign x 1)
  (assert (not ((get probes 0))) "the probe runs after the write"))

# A written parameter holds the caller's value until the write.
(defn fresh? [@x]
  "Whether x was nil before the function wrote it."
  (def was (nil? x))
  (assign x 1)
  was)
(assert (fresh? nil) "the read before the write sees the caller's nil")

# A divisor the call may zero is not provably nonzero.
(defn compiles? [src]
  "Whether src compiles."
  (first (protect (compile/whole-module src "<guard-facts>"))))
(assert (not (compiles? "(var d 1)
                         (defn zero! [] (assign d 0))
                         (defn q [] (if (%ne d 0) (begin (zero!) (%div 10 d)) 0))
                         (q)"))
        "the call may have set d to zero")
