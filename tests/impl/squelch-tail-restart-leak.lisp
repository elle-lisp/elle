(elle/epoch 13)
# audited: 2026-09-29
# A restart after a squelch at a tail call runs the releases the dropped activation owed, as a return would.
# docs/impl/region/unwind.md
#
# tests/lang/squelch-tail-restart.lisp pins the values such a restart answers.
# This file gauges what it leaves: `arena/count` and `arena/region-count`
# deltas over a fixed window. The control restarts a callee's tail call on the
# first run, which drops the same frames and leaves the body to run on. The
# counter-factual is a restart that skips the dropped activation's releases,
# and each face then costs objects and regions beyond the control.

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

(defn relay []
  ((squelch bound-yield :yield)))

(defn measure [body]
  (var i 0)
  (while (%lt i 20)
    (restart body)
    (assign i (%add i 1)))
  (def objects (arena/count))
  (def regions (arena/region-count))
  (var j 0)
  (while (%lt j 300)
    (restart body)
    (assign j (%add j 1)))
  [(%sub (arena/count) objects) (%sub (arena/region-count) regions)])

(def d-control (measure (fn [] (list :got (relay)))))
(def d-body (measure (fn [] ((squelch bound-yield :yield)))))
(def d-resumed
  (measure (fn []
             (yield 0)
             ((squelch bound-yield :yield)))))
(println "squelch-tail-restart-leak [objects regions]: control " d-control
         " body " d-body " resumed " d-resumed)
(assert (< (- (get d-body 0) (get d-control 0)) 50)
        "a body's restart leaves no objects behind")
(assert (< (- (get d-body 1) (get d-control 1)) 50)
        "a body's restart leaves no regions behind")
(assert (< (- (get d-resumed 0) (get d-control 0)) 50)
        "a resumed body's restart leaves no objects behind")
(assert (< (- (get d-resumed 1) (get d-control 1)) 50)
        "a resumed body's restart leaves no regions behind")

(println "squelch-tail-restart-leak: OK")
