(elle/epoch 9)

# ── flat cond ──────────────────────────────────────────────

(let [x 5]
  (assert (= (cond
               (> x 10) :large
               (> x 0)  :small
               :negative)
             :small)
          "flat cond basic"))

(let [x -3]
  (assert (= (cond
               (> x 10) :large
               (> x 0)  :small
               :negative)
             :negative)
          "flat cond default"))

(assert (= (cond false 1 true 2) 2) "flat cond second clause")
(assert (nil? (cond false 1 false 2)) "flat cond no match returns nil")

# ── flat match ─────────────────────────────────────────────

(assert (= (match 42
             :quit :done
             42    :found
             _     :other)
           :found)
        "flat match literal")

(assert (= (match [1 2]
             [a b] (+ a b)
             _     0)
           3)
        "flat match destructure")

(assert (= (match 5
             x when (> x 0) :positive
             _               :other)
           :positive)
        "flat match guard")

(assert (= (match -3
             x when (> x 0) :positive
             _               :other)
           :other)
        "flat match guard fallthrough")

(println "flatcond: all tests passed")
