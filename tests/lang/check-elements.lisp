(elle/epoch 18)
# audited: 2026-11-20
# std/check: the array a repeated draw makes, the bits each element reads, and the shrinker deleting elements.
# docs/stdlib/check.md
#
# The counter-factual: a shrinker that deletes one draw's bits cannot remove
# an element made of two draws. An array of pairs that failed on one pair kept
# simplest pairs in front of it, [[0 0] [0 0] [501 501]], and still met every
# claim that asks only for a failing case. Each shrinking test here names the
# one smallest failure.

(def check ((import "std/check")))
(include-file "check/parts.lisp")

(defn replayed [bits g]
  "The value G makes from a replay of BITS, and the record of the replay."
  (let [src (source:replay bits)
        v (g (src :draw))]
    [v ((src :record))]))

(include-file "check/helpers.lisp")

# ── what (draw g lo hi) makes ─────────────────────────────────────────
(def digits (fn [draw] (draw (check:int 0 9) 1 3)))
(assert (= [[0] [1] [2] [0 0] [0 1] [1 0] [0 0 0]] (check:enumerate digits 2))
        "a repeated draw draws its length, then each element")
(assert (= (check:enumerate digits 4)
           (check:enumerate (check:array-of (check:int 0 9) 3 :min 1) 4))
        "array-of makes the arrays a repeated draw makes")
(assert (= [[false false] [false true] [true false]]
           (check:enumerate (fn [draw] (draw (check:bool) 2 2)) 1))
        "a length with one value reads no bits")
(assert (= :array (type-of (get (check:enumerate digits 0) 0)))
        "the array is immutable")
(each xs in (check:sample digits :seed 4 :count 50)
  (assert (and (<= 1 (length xs)) (<= (length xs) 3))
          "a sampled array keeps its length from lo to hi"))

(def any-draw ((source:replay []) :draw))
(each [lo hi why] in [[3 2 "a repeated draw refuses lo above hi"]
                      [-1 2 "and a negative lo"]
                      [0 1.5 "and a bound that is not an integer"]
                      [2 nil "and a generator with no hi"]]
  (let [err (raises (fn [] (any-draw (check:bool) lo hi)))]
    (assert (= :argument-error (err :error)) why)))

# ── the bits each element reads ───────────────────────────────────────
(def pair (check:tuple (check:int 0 1000) (check:int 0 1000)))
(def pairs (fn [draw] (draw pair 0 6)))
(def bits-501 [0 1 1 1 1 1 0 1 0 1])

(let [[v record] (replayed [0 0 1 ;bits-501 ;bits-501] pairs)]
  (assert (= [[501 501]] v) "three bits for a length to 6, ten for each int")
  (assert (= [[3 23]] (get (record :elements) 0))
          "the length draw records the bits of its one element"))

(let [[v record] (replayed [0 1 1 0 1 0]
                           (fn [draw] (draw (fn [d] (d (check:bool) 0 2)) 0 2)))]
  (assert (= [[true false]] v) "an array of one array of two bools")
  (assert (= [[2 6]] (get (record :elements) 0))
          "the outer element holds every bit of the inner array")
  (assert (= [[4 5] [5 6]] (get (record :elements) 1))
          "and the inner length draw records its own elements"))

(let [[v record] (replayed [1 1] (fn [draw] (draw (check:constant :x) 0 3)))]
  (assert (= [:x :x :x] v) "an element that draws nothing")
  (assert (= [[2 2] [2 2] [2 2]] (get (record :elements) 0)) "reads no bits"))

# ── the shrinker deletes one element ──────────────────────────────────
(defn big-pair? [[a b]]
  (and (> a 500) (> b 500)))

(each seed in (range 5)
  (let [r (check:run (check:array-of pair 6) (fn [xs] (not (any? big-pair? xs)))
                     :depth 0 :seed seed)]
    (assert (= [[501 501]] (r :value))
            (string "seed " seed ": the pairs in front of the failure go"))
    (assert (= [1 501 501] (r :choices))
            "one draw for the length, two for the pair")
    (assert (= [0 0 1 ;bits-501 ;bits-501] (r :bits))
            "and the bits of the length are written one less each time")))

(let [r (check:run pairs (fn [xs] (not (any? big-pair? xs))) :depth 0 :seed 1)]
  (assert (= [[501 501]] (r :value))
          "a generator of your own that draws its array gets the same deletion"))

# The counter-factual: without the swap step, two cases at the :min met the
# shrinker's rule, one with the failing pair first and one with it last.
(each seed in (range 5)
  (let [r (check:run (check:array-of pair 6 :min 2)
                     (fn [xs] (not (any? big-pair? xs))) :depth 0 :seed seed)]
    (assert (= [[0 0] [501 501]] (r :value))
            (string "seed " seed
                    ": an array never shrinks under its :min, simplest first"))
    (assert (= 0 (get (r :choices) 0)) "its length choice is 0")))

(defn big-then-big? [xs]
  "Whether a pair whose first is over 500 precedes a pair whose second is."
  (var found false)
  (each i in (range (length xs))
    (each j in (range (+ i 1) (length xs))
      (when (and (> (get (get xs i) 0) 500) (> (get (get xs j) 1) 500))
        (assign found true))))
  found)

(each seed in (range 5)
  (let [r (check:run (check:array-of pair 6) (fn [xs] (not (big-then-big? xs)))
                     :depth 0 :seed seed)]
    (assert (not (r :passed)) (string "seed " seed " finds a failure"))
    (assert (= [[501 0] [0 501]] (r :value))
            (string "seed " seed ": deletion keeps the elements on each side"))))

(def item
  (check:tuple (check:int 0 1000) (check:array-of (check:bool) 5 :min 1)))
(each seed in (range 5)
  (let [r (check:run (check:array-of item 5)
                     (fn [xs]
                       (not (any? (fn [[v s]] (and (> v 500) (> (length s) 3)))
                                  xs))) :depth 0 :seed seed)]
    (assert (= [[501 [false false false false]]] (r :value))
            (string "seed " seed ": nested arrays shrink to one item of four"))
    (assert (= [1 501 3 0 0 0 0] (r :choices)) "the choices that make it")
    (assert (= (r :value)
               ((check:array-of item 5) ((source:replay (r :bits)) :draw)))
            "a replay of :bits makes :value")))

(each seed in (range 5)
  (let [r (check:run (check:array-of (check:constant :x) 5)
                     (fn [xs] (< (length xs) 2)) :depth 0 :seed seed)]
    (assert (= [:x :x] (r :value))
            (string "seed " seed
                    ": elements of no bits shrink to the shortest failure"))
    (assert (= [0 1 0] (r :bits)) "the three bits of a length choice of 2")))

# ── the shrinker deletes a run of elements ────────────────────────────
# The counter-factual: deleting one element at a time can stop at [0 0 501].
# Either 0 alone leaves an even length, which passes, and a lower length
# cuts the 501 from the end.
(defn odd-ending-big? [xs]
  (and (odd? (length xs)) (> (get xs -1) 500)))

(each seed in (range 6)
  (let [r (check:run (check:array-of (check:int 0 1000) 8)
                     (fn [xs] (not (odd-ending-big? xs))) :depth 0 :seed seed)]
    (assert (= [501] (r :value))
            (string "seed " seed ": two elements in front of it go together"))))

(each seed in (range 6)
  (let [r (check:run (check:array-of (check:int 0 1000) 8 :min 3)
                     (fn [xs] (not (odd-ending-big? xs))) :depth 0 :seed seed)]
    (assert (= [0 0 501] (r :value))
            (string "seed " seed ": a run never takes the array under :min"))))

(println "check-elements: all tests passed")
