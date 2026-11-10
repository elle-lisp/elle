(elle/epoch 15)
# audited: 2026-10-21
# A pair holding a region the merge seed leaves apart counts that region once, so the region frees with the pair.
#
# docs/impl/region/mechanism.md
#
# The sidecar keeps the JIT off and turns the graft off, so each `%pair` reaches
# the region solver as written. Guardfree is armed too: a count taken away from
# the field must not free it under a later read.
#
# Each face stores a fresh heap value into a `%pair` and reads the value again
# after the pair's own last use. That read refuses the builder-idiom merge, so
# the value keeps a region of its own, and the pair's allocation scan counts the
# pair's reference to it. The pair's free-time cascade releases that count.
#
# The counter-factual: a store edge that also took a static `IncrefRegion` would
# take it once for the walk and once more for each inline of the function's
# body. Nothing releases those, so the stored value's region, and everything it
# holds, would outlive each call.

(defn nest-read [i]
  (let [inner (%pair i ())
        outer (%pair i inner)]
    (%first outer)
    (%first inner)))

(defn literal-read [i]
  (let [s "lit"
        p (%pair i s)]
    (%first p)
    (length s)))

# A list walked by a reassigned cursor: the cursor holds the outer pair, and
# the inner pair is the outer pair's cdr.
(defn cursor-walk [i]
  (let [xs (%pair (string "a" i) (%pair (string "b" i) ()))]
    (def @p xs)
    (def @n 0)
    (while (%pair? p)
      (assign n (+ n (length (%first p))))
      (assign p (%rest p)))
    n))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (assert (= (nest-read i) i) "nest-read reads the inner pair")
    (assert (= (literal-read i) 3) "literal-read reads the literal")
    (assert (= (cursor-walk i) (* 2 (length (string "a" i))))
            "cursor-walk reads both pairs")
    (assign i (+ i 1))))

(churn 500)

(def r0 (arena/region-count))
(churn 500)
(def growth (- (arena/region-count) r0))
(assert (< growth 50)
        (string "pair edges leaked " growth " regions over 500 ops"))

(println "region-pair-edge-leak: ok")
