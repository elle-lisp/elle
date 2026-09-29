(elle/epoch 13)
# audited: 2026-09-29
# A suspension `compile/run-on :jit` refuses ends its park.
# docs/impl/region/park.md
#
# The forced JIT tier cannot host a suspension, so it answers a park with a
# :tier-rejected error at its own call. The park that raised the signal is
# over, and the host ends it (`refuse_hosted_park`). The JIT
# tier answers a park on two arms: its own code yielded, or a bytecode tail
# callee did. Each case below reaches one arm.
#
# The counter-factual is a refusal that leaves the funding standing. The error
# then leaves the protect fiber from tail position, the driver builds its error
# park, and the delivery ledger rejects an unconsumed park (a panic in a debug
# build). A release build would mint a resume reference that nothing releases.
#
# The refusal may be the payload's last release, and a host that reads the
# payload afterwards, to describe it in its error, reads a freed page. A run
# catches that only where the page is reused first, as on macOS, so the
# sidecar arms guardfree, which faults on it everywhere.

(def _jit-available
  (let [[ok? v] (protect (compile/run-on :jit (fn [] 0)))]
    (if (and (not ok?) (= (get v :error) :tier-rejected))
      (error (struct :error :gated :reason "JIT tier not compiled in"))
      true)))

(defn refused? [r]
  (and (not (get r 0)) (= (get (get r 1) :error) :tier-rejected)
       (= (get (get r 1) :reason) :ineligible)))

# The JIT-compiled thunk itself parks, at a tail io call.
(assert (refused? (protect (compile/run-on :jit (fn [] (ev/sleep 0)))))
        "a thunk that parks is refused")

# The thunk tail-calls a bytecode closure, and that closure parks.
(defn sleeper []
  (ev/sleep 0))
(assert (refused? (protect (compile/run-on :jit (fn [] (sleeper)))))
        "a tail callee that parks is refused")

# The fiber runs on after both refusals.
(assert (= (ev/sleep 0) nil) "io still works after the refusals")

# A refusal is an error at the `compile/run-on` call, and the thunk's parked
# frames are dropped. A restart answers the call. The counter-factual keeps the
# thunk's frames parked: the restart replays the thunk, and its value ends the
# fiber in place of the body.
(def restarted
  (fiber/new (fn [] (list :got (compile/run-on :jit (fn [] (+ 1 (yield 1))))))
             |:yield :error|))
(assert (refused? [false (fiber/resume restarted)])
        "the thunk's yield is refused")
(assert (= (fiber/resume restarted 41) (list :got 41))
        "a restart answers the compile/run-on call")

# A refusal costs no more than a raise at the same call. The counter-factual
# only clears the ledger, and the thunk's frames and the park's delivery
# retain go with the dropped fiber: regions and objects past the control.
(defn measure [thunk]
  (var i 0)
  (while (%lt i 20)
    (thunk)
    (assign i (%add i 1)))
  (def objects (arena/count))
  (def regions (arena/region-count))
  (var j 0)
  (while (%lt j 300)
    (thunk)
    (assign j (%add j 1)))
  [(%sub (arena/count) objects) (%sub (arena/region-count) regions)])

(def d-control
  (measure (fn []
             (protect (compile/run-on :jit (fn [] (+ 1 (error (string "y" 1)))))))))
(def d-refused
  (measure (fn []
             (protect (compile/run-on :jit (fn [] (+ 1 (yield (string "y" 1)))))))))
(println "jit-run-on-refused-park [objects regions]: control " d-control
         " refused " d-refused)
(assert (< (- (get d-refused 0) (get d-control 0)) 50)
        "a refusal leaves no objects behind")
(assert (< (- (get d-refused 1) (get d-control 1)) 50)
        "a refusal leaves no regions behind")

(println "jit-run-on-refused-park: ok")
