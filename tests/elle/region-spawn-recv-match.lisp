(elle/epoch 12)
## Reproducer: spawn child + send + recv-match with array? predicate.
##
## Crashes (segfault / "Cannot call <closure>" with type=ffi-signature) at
## stdlib:1884 in ev/run when calling ((get sched :pump)). The pump closure
## was allocated inside make-async-scheduler's let body and gets freed
## before the implicit top-level ev/run pumps the spawned fiber.
##
## Minimal shape — three things matter:
##   - process:spawn (spawns a child fiber that yields)
##   - process:send  (yields a message to the scheduler)
##   - process:recv-match with a CLOSURE predicate (yields the closure to
##     the scheduler; the scheduler retains it across pump iterations and
##     re-invokes it as new messages arrive)

(def process ((import-file "lib/process.lisp")))
(def backend (*io-backend*))

(defn process:start [init &named fuel]
  ((get process :start) init :fuel fuel :backend backend))

(process:start (fn []
                 (let [me ((get process :self))]
                   ((get process :send) ((get process :spawn) (fn []
                                          (let [m ((get process :recv))]
                                            ((get process :send) (get m 0)
                                            [:reply :tag :pong])))) [me])
                   ((get process :recv-match) (fn [m]
                     (and (array? m) (= (get m 0) :reply)))))))

(println "region-spawn-recv-match: PASS")
