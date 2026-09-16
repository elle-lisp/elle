(elle/epoch 12)
# audited: 2026-09-16
# An allocation in the BODY of a `let` is released, wherever the let's value goes.
#
# The lowerer releases a value through the slot of the binding that names it,
# and it records that slot against the region the binding's INIT NODE allocates.
# A `let` allocates nothing at its own id — it hands its body's value up — so a
# name on the `let` records no slot and the release the solver placed emits no
# instruction. The value is held to fiber teardown: one region per evaluation.
#
# Two positions receive such a value, and both leaked:
#
#   - a consumer, `(length (let [x 7] [x x]))`;
#   - a binder, `(let [inner (let [x 7] [x x])] …)` and the `def` form of it.
#
# The fused `mapcat` is the second one at scale: its inner walk binds the array
# its function returned to a loop-local whose init is exactly that shape, so the
# rate was one region per INPUT element per call (elle-lisp/elle#1121).
#
# The discriminators must already read bounded, and they say what the defect was
# not: an array literal in the same consumer position has its own name and is
# reclaimed, and the same `mapcat` chain through a parameter declines fusion and
# runs the stdlib op, which allocates the same per-element arrays and frees them.

(def a [1 2 3 4 5])

(defn consumer (z)
  (length (let [x 7]
            [x x])))
(defn binder (z)
  (let [inner (let [x 7]
                [x x])]
    (length inner)))
(defn defined (z)
  (def d
    (let [x 7]
      [x x]))
  (length d))
(defn fused (z)
  (mapcat (fn (y) [y y]) a))

# The two discriminators.
(defn named-already (z)
  (length [7 7]))
(defn through-param (f coll)
  (mapcat f coll))
(defn declined (z)
  (through-param (fn (y) [y y]) a))

(defn churn (f iters)
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# Each row is (label fn), and each must stay bounded as the iteration count
# grows by 10x. A rate of one region per call reads 100 at n=100 and 1000 at
# n=1000, so a ceiling of 20 separates a bounded shape from every rate here.
(each row in [["consumer" consumer] ["binder" binder] ["defined" defined]
              ["fused mapcat" fused] ["named already" named-already]
              ["declined mapcat" declined]]
  (let [label (get row 0)
        f (get row 1)
        d100 (churn f 100)
        d1000 (churn f 1000)]
    (assert (%lt d100 20)
            (concat label ": region leak at n=100: delta=" (number->string d100)))
    (assert (%lt d1000 20)
            (concat label ": region leak at n=1000: delta="
                    (number->string d1000)))))
