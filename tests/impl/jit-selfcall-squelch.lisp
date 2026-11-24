(elle/epoch 18)
# audited: 2026-11-23
# A squelched closure that calls itself checks its squelch at every level, so a yield deep inside fails at the innermost call.
# docs/impl/jit-selfcall.md
#
# `walk` calls itself with constant arguments, which are immediates, so the
# compiler admits each call as a direct self-call. `safe` is `walk` under a
# squelch of `:yield`, and the running closure is `safe` at every level. So
# each level's call meets the squelch, and the guard sends it through the
# dispatch helper, which checks it.
#
# The yield at the bottom fails at the innermost boundary, in the call from
# level 1. The error parks levels 1 to 3, and a resume with 100 restarts that
# call: each level adds one, and the fiber answers 103.
#
# The counter-factual: a direct call with no squelch guard. The yield would
# cross every level as a yield, fail at the outermost call, and park only the
# thunk, so the resume would answer 100.

(defn walk [n]
  (cond
    (= n 0) (yield 1)
    (= n 1) (+ 1 (walk 0))
    (= n 2) (+ 1 (walk 1))
    true (+ 1 (walk 2))))

(def safe (squelch :yield walk))

(def f (fiber/new (fn [] (safe 3)) |:error|))
(def failed (fiber/resume f))
(assert (= (fiber/status f) :paused) "the error parks the fiber")
(assert (= (get failed :error) :signal-violation)
        "the squelch turns the yield into an error")
(assert (= (fiber/resume f 100) 103)
        "the resume restarts the innermost call, under three levels")

(println "jit-selfcall-squelch: ok")
