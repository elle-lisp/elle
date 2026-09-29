(elle/epoch 12)
# audited: 2026-09-23
# A fiber discarded while parked inside a variadic callee releases the rest list the calling convention built.
# docs/impl/region/owner.md
#
# The rest parameter's slot is a value route with a receipt, so the discharge
# of a dropped parked fiber releases the list off the frame's release table.
# Counterfactual: a rest list that no table names strands its three conses and
# their elements once per discarded fiber, and the object count grows with
# the number of fibers dropped.
#
# The fixed-arity callee is the control: the same arguments, the same park,
# and no rest list, so a harness that cannot see a discharge fails it too.

(defn variadic [& xs]
  (yield 1)
  (length xs))

(defn fixed [a b c]
  (yield 1)
  (length (list a b c)))

(defn drop-parked [call n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new call |:yield|)]
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(def variadic-call (fn [] (variadic (list 1 2) (list 3 4) (list 5 6))))
(def fixed-call (fn [] (fixed (list 1 2) (list 3 4) (list 5 6))))

(drop-parked variadic-call 200)
(drop-parked fixed-call 200)

(let [d1k (drop-parked fixed-call 1000)
      d4k (drop-parked fixed-call 4000)]
  (assert (and (%lt d1k 100) (%lt d4k 100))
          (string "control: a dropped fixed-arity park strands objects, d1k="
                  d1k " d4k=" d4k)))

(let [d1k (drop-parked variadic-call 1000)
      d4k (drop-parked variadic-call 4000)]
  (assert (and (%lt d1k 100) (%lt d4k 100))
          (string "a dropped variadic park strands its rest list, d1k=" d1k
                  " d4k=" d4k)))

(println "region-discard-variadic-rest: ok")
