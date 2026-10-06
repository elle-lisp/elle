(elle/epoch 15)
# audited: 2026-10-06
# A dynamic emit requires the bits its first argument names, so a fiber cannot raise a bit it withholds.
# docs/signals/emit.md
#
# The dynamic form, whose first argument is not a literal keyword, is a call to
# the `fiber/emit` primitive, and its requirement rides that argument through
# the same bits_from_args gate `io/submit` uses. caps-emit.lisp holds the
# literal form.

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
