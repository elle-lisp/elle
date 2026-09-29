(elle/epoch 13)
# audited: 2026-09-29
# A restart answers the call whose allocation the object limit refused.
# docs/signals/primitives.md
#
# The object limit (`arena/set-object-limit`) is an extension of this
# implementation, so this face of tests/lang/fiber-restart.lisp lives here.
# The limit refuses the allocation past it: the call that asked gets nil back
# from the heap, and the loop raises for that call right after it. A restart
# answers the call.
#
# The counter-factual is a restart that continues after the call and binds its
# nil. The trap: the limit is heap-wide, so the fiber lifts it itself, and the
# resumer allocates nothing between the raise and the restart.

(def over-limit
  (fiber/new (fn []
               (arena/set-object-limit (+ (arena/count) 3))
               (let [xs (list 1 2 3 4 5 6 7 8)]
                 (arena/set-object-limit nil)
                 (list :got xs))) |:error|))
(def limit-error (fiber/resume over-limit))
(def limit-restart (fiber/resume over-limit :ignored))
(assert (= :allocation-error (get limit-error :error)) "the object limit raises")
(assert (= (list :got :ignored) limit-restart)
        "the restart answers the call whose allocation the limit refused")

(println "fiber-restart-object-limit: OK")
