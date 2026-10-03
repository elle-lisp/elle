(elle/epoch 13)
# audited: 2026-09-29
# A restart after a squelch at a tail call answers the call to the function the tail call replaced.
# docs/signals/primitives.md
#
# The tail call has replaced the calling function by the time the boundary
# refuses, so the refusal drops both, and no frame of the calling function is
# left to restart. The counter-factual parks the tail callee's code at its
# yield over a drained stack: the restart panics in a debug build, and runs
# the callee's code on garbage in a release build.
# tests/impl/squelch-tail-restart-leak.lisp gauges what the restart leaves.

(def depth (make-parameter 0))
(defn bound-yield []
  (parameterize ((depth 9))
    (yield (string "y" 1))
    :inner))

(defn restart [body]
  "Run `body` in a fiber until the refusal stops it, restart it with 41, and
  answer the fiber's value and status."
  (let [f (fiber/new body |:yield :error|)]
    (var v (fiber/resume f))
    (while (not (= (fiber/bits f) 1)) (assign v (fiber/resume f)))
    (assert (= (get v :error) :signal-violation)
            "the boundary stops the fiber on its violation")
    [(fiber/resume f 41) (fiber/status f)]))

## ── a callee's tail call ────────────────────────────────────────────

(defn relay []
  ((squelch bound-yield :yield)))
(defn yield-relay []
  (yield 0)
  ((squelch bound-yield :yield)))

(assert (= (restart (fn [] (list :got (relay) (depth))))
           [(list :got 41 0) :dead])
        "a restart answers the call to the function the tail call replaced")

(assert (= (restart (fn [] (list :got (yield-relay) (depth))))
           [(list :got 41 0) :dead])
        "so it does when that function suspended before its tail call")

## ── the fiber body's tail call ──────────────────────────────────────

(assert (= (restart (fn [] ((squelch bound-yield :yield)))) [41 :dead])
        "a body's recovery value is the fiber's result")

(assert (= (restart (fn []
                      (yield 0)
                      ((squelch bound-yield :yield)))) [41 :dead])
        "so it is when the body suspended before its tail call")

(assert (= (restart (fn []
                      (parameterize ((depth 5))
                        (yield 0))
                      ((squelch bound-yield :yield)))) [41 :dead])
        "a body that bound and unbound a parameter ends the same way")

(let [f (fiber/new (fn [] ((squelch bound-yield :yield))) |:yield :error|)]
  (fiber/resume f)
  (fiber/resume f 41)
  (assert (= (fiber/value f) 41) "the fiber holds the recovery value")
  (assert (= (fiber/bits f) 0) "as a return, with no signal bits"))
