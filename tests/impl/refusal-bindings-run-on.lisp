(elle/epoch 13)
# audited: 2026-09-29
# A park `compile/run-on` refuses drops the `parameterize` bindings its code made, as the language's refusals do.
# docs/signals/primitives.md
#
# tests/lang/refusal-bindings.lisp pins the rule for a squelch boundary, `eval`
# and `import`. `compile/run-on` is an extension of this implementation, so its
# faces live here: a squelch under the forced bytecode and JIT tiers, a park
# the forced JIT tier refuses, and an error the compiled closure raises. The
# counter-factual leaves the refused frames' bindings on the fiber: a restart
# answers the refused call, and the code after it reads the refused code's
# binding (9) instead of its own.

(def depth (make-parameter 0))
(defn bound-yield []
  (parameterize ((depth 9))
    (yield 1)
    :inner))
(defn bound-raise []
  (parameterize ((depth 9))
    (+ 1 (error :boom))))

(defn restart [body]
  "Run `body` in a fiber that stops on the refusal, restart it with 41, and
  answer what the body ends with."
  (let [f (fiber/new body |:yield :error|)]
    (fiber/resume f)
    (assert (= (fiber/bits f) 1) "the refusal stops the fiber on an error")
    (fiber/resume f 41)))

(def squelched (squelch bound-yield :yield))

(assert (= (restart (fn []
                      (list :got (compile/run-on :bytecode squelched) (depth))))
           (list :got 41 0))
        "a squelch under compile/run-on :bytecode drops the refused binding")

# A build without the JIT tier answers every :jit request :tier-rejected, so
# the cases below run where the tier is compiled in.
(def jit-tier?
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (not (and (not ok?) (= (get v :error) :tier-rejected)))))

(when jit-tier?
  (assert (= (restart (fn [] (list :got (compile/run-on :jit squelched) (depth))))
             (list :got 41 0))
          "a squelch under compile/run-on :jit drops the refused binding")
  (assert (= (restart (fn []
                        (list :got (compile/run-on :jit bound-yield) (depth))))
             (list :got 41 0)) "compile/run-on :jit drops the refused binding")
  # The compiled closure's frames leave by the error and pop no binding, so the
  # host truncates to the depth at its entry, as every other host does.
  (assert (= (restart (fn []
                        (list :got (compile/run-on :jit bound-raise) (depth))))
             (list :got 41 0))
          "an error under compile/run-on :jit drops the raising code's binding"))
