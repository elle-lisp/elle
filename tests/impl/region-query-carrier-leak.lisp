(elle/epoch 14)
# audited: 2026-10-06
# A query whose answer does not live in the call's region frees the pair that asked it.
# docs/impl/region/ctx.md
#
# A query primitive returns `(op . arg)` born in its call's fresh region, and
# the VM answers it there. The counter-factual leaves that region to the
# caller's release, which names the answer. An immediate or borrowed answer
# names no region, so the pair is stranded on every call: one object per
# `(vm/config :max-depth)`, two per `(vm/config-set ...)`, whose argument is a
# second pair. A fresh answer shares the region and frees the pair with it,
# which is the control.

(def calls 2000)

(defn growth [thunk]
  "Objects left on the heap by `calls` runs of `thunk`, after a warm-up."
  (def @i 0)
  (while (%lt i 200)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (def @j 0)
  (while (%lt j calls)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(defn bounded [label thunk]
  (let [g (growth thunk)]
    (assert (< g 20) (string label ": " g " objects over " calls " calls"))))

# The gauge moves: a shape that keeps every call's value reads as growth.
(def @kept @[])
(assert (>= (growth (fn [] (push kept (string "k" 1)))) calls)
        "a kept value reads as growth (gauge dead)")
(assign kept @[])

(defn counted []
  1)

(bounded "fresh answer (control)"
         (fn []
           (vm/config :unicode)
           nil))
(bounded "int answer"
         (fn []
           (vm/config :max-depth)
           nil))
(bounded "bool answer"
         (fn []
           (vm/config :stats)
           nil))
(bounded "nil answer to a nested pair"
         (fn []
           (vm/config-set :stats false)
           nil))
(bounded "call-count"
         (fn []
           (call-count counted)
           nil))
(bounded "jit?"
         (fn []
           (jit? counted)
           nil))

# A borrowed answer: inside a fiber, `fiber/self` answers the running fiber,
# which lives in a region the call did not mint.
(def in-fiber
  (fiber/new (fn []
               (growth (fn []
                         (vm/query "fiber/self" nil)
                         nil))) |:yield|))
(let [g (fiber/resume in-fiber)]
  (assert (< g 20)
          (string "borrowed answer: " g " objects over " calls " calls")))

(println "region-query-carrier-leak: ok")
