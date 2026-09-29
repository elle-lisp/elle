(elle/epoch 12)
# Regression for compile/barrier-module — the per-form fault-barrier test
# compilation mode behind `elle test` (docs/test-runner.md § Mechanism).
#
# The whole file compiles as ONE module: def/var forms run eagerly to establish
# shared bindings, and each test (expression) form is returned as a 0-arg thunk
# capturing that environment. This exercises the contract directly (the runner
# acceptance harness, tests/runner/acceptance.lisp, exercises it end-to-end).

# Run a thunk on the bytecode tier, fault-isolated, → [ok? payload].
(defn run-bc [thunk]
  (protect (compile/run-on :bytecode thunk)))

# A multi-form module: a def, two asserts (one failing), a defn AFTER the
# failing assert, and an assert that uses BOTH the earlier def and the later
# defn — impossible under independent per-form compilation.
(def src
  "(def base 41)
(assert (= base 41) \"reads earlier def\")
(assert (= (+ base 1) 99) \"intentional fail\")
(defn helper [x] (+ x base))
(assert (= (helper 1) 42) \"uses later defn and earlier def\")")

(let [out (compile/barrier-module src "<barrier-test>")]
  (assert (= (length out) 3)
          (string "expected 3 test-form entries (def/defn produce none), got "
                  (length out)))
  (assert (= (get (get out 0) 0) 1) "first test form at index 1")
  (assert (= (get (get out 1) 0) 2) "second test form at index 2")
  (assert (= (get (get out 2) 0) 4) "third test form at index 4")

  # form 1 reads an earlier def → passes (shared bindings)
  (assert (get (run-bc (get (get out 0) 1)) 0)
          "form reading an earlier def must pass")

  # form 2 is the intentional failure: caught (does not abort siblings), and the
  # typed payload carries the predicate operands evaluated against the shared env.
  (let [r (run-bc (get (get out 1) 1))]
    (assert (not (get r 0)) "the failing form is caught as data")
    (assert (= (get (get r 1) :error) :failed-assertion)
            "failure keeps the typed :failed-assertion signal")
    (assert (= (get (get r 1) :actual) 42)
            "the payload's actual is the shared binding's value (base+1)"))

  # form 3 uses a defn declared AFTER the failing form, plus the earlier def
  (assert (get (run-bc (get (get out 2) 1)) 0)
          "form using a later defn + earlier def must pass (forward + shared)"))

# A file that won't compile signals a structured :compile-error (the runner
# turns this into a single file-level failure rather than per-form rows).
(let [r (protect (compile/barrier-module "(no-such-binding-xyz 1)"
                 "<barrier-test>"))]
  (assert (not (get r 0)) "a non-compiling file signals rather than returning")
  (assert (= (get (get r 1) :error) :compile-error)
          (string "expected :compile-error, got " (get r 1))))

(println "barrier-module tests passed")
