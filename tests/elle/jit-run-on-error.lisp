(elle/epoch 13)
# audited: 2026-09-29
# An error a closure raises under `compile/run-on :jit` reaches the caller as that error.
#
# docs/impl/differential.md
#
# Compiled code leaves an `(error …)` through the yield side exit, as it
# leaves any emit. The counter-factual reads that exit as a suspension and
# answers :tier-rejected, "closure yielded under compile/run-on", in place of
# the closure's own error.

(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

(defn boom? [r]
  (and (not (get r 0)) (= (get (get r 1) :error) :boom)))

(assert (boom? (protect (compile/run-on :jit (fn []
                                          (error {:error :boom :message "b"})))))
        "a raise in the compiled closure is the closure's own error")

(assert (boom? (protect (compile/run-on :jit (fn [x]
                                          (if x
                                            (error {:error :boom :message "b"})
                                            1)) true)))
        "so is a raise on one branch of it")

(assert (= (compile/run-on :jit (fn [x]
                                  (if x (error {:error :boom :message "b"}) 1))
                           false) 1) "the other branch still returns its value")

# The error stops a fiber at the `compile/run-on` call, as any raise at a call
# does, and a restart answers that call.
(def raised
  (fiber/new (fn []
               (list :got (compile/run-on :jit (fn []
                       (error {:error :boom :message "b"}))))) |:error|))
(assert (= (get (fiber/resume raised) :error) :boom)
        "the fiber stops on the closure's error")
(assert (= (fiber/bits raised) 1) "and on the error bit alone")
(assert (= (fiber/resume raised 41) (list :got 41))
        "a restart answers the compile/run-on call")
