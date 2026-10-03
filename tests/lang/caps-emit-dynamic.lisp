(elle/epoch 12)
# audited: 2026-09-21
# ── dynamic emit requires the bits it raises (#1073, dynamic half) ─────
#
# A fiber raised any signal bit it named. The literal `emit` is a bytecode
# instruction and still does — that is a separate gate. The DYNAMIC form, whose
# first argument is not a literal keyword, routes through the `fiber/emit`
# primitive, and its requirement now rides that argument through the same
# bits_from_args gate `io/submit` uses. So a fiber cannot dynamically emit a
# capability it withholds.

# `k` is a variable, so `(emit k …)` is the dynamic form, not the special form.
# Counterfactual: `fiber/emit` declares `:yield`/`:error`, so without the
# derived requirement `:deny |:exec|` does not stop it and the fiber parks on
# the raw emitted payload rather than a denial.
(let [k :exec
      f (fiber/new (fn []
                     (emit k "not a request")
                     :ran) |:exec :error| :deny |:exec|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "dynamic (emit :exec …) under :deny |:exec| is denied")
  (let [v (fiber/value f)]
    (assert (= :capability-denied (get v :error))
            "the denial is a capability denial")
    (assert ((get v :denied) :exec) "the denial names :exec, the emitted bit")))

# A bit the fiber holds is emitted freely: dynamic emit of :yield, which nobody
# withheld, suspends as an ordinary yield and delivers its value on resume.
(let [k :yield
      f (fiber/new (fn [] (emit k 99)) |:yield :error|)]
  (assert (= 99 (fiber/resume f)) "dynamic (emit :yield 99) yields its value")
  (assert (= (fiber/status f) :paused)
          "the yielding fiber is paused, not denied"))

(println "caps-emit-dynamic: OK")
