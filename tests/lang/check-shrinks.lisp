(elle/epoch 18)
# audited: 2026-11-20
# std/check: the cap on the candidates the shrinker runs, and the swap of two elements.
# docs/stdlib/check.md
#
# The counter-factual: a shrinker that deletes and lowers alone keeps the
# order it found a case in, so [5 0 0] stays [5 0 0]. A cap that counts
# something other than candidates still stops a long shrink, so the cap tests
# count the property's calls.

(def check ((import "std/check")))

(include-file "check/helpers.lisp")

(def g (check:int 0 1000))
(defn below-17 [n]
  (< n 17))
(def bits-1000 [1 1 1 1 1 0 1 0 0 0])

# ── the cap on candidates ─────────────────────────────────────────────
(let [r (check:run g below-17 :bits bits-1000 :shrinks 0)]
  (assert (= [1000 false] [(r :value) (r :minimal)])
          "a cap of 0 keeps the case it was given"))

(assert (= true (get (check:run g below-17) :minimal))
        "a shrinker that finishes under the cap is minimal")

(var calls 0)
(defn counted [prop]
  "PROP, counting each call in CALLS."
  (fn [x]
    (assign calls (+ calls 1))
    (prop x)))

# Each of the five bools is a one-bit draw. Deleting one is a shorter
# candidate, and its replay reads a 0 past the end, so none is kept. The
# first case is all false, so no other step has a candidate.
(def five (check:array-of (check:bool) 5 :min 5))
(let [r (check:run five (counted (fn [_] false)) :shrinks 5)]
  (assert (= [true 6] [(r :minimal) calls])
          "a cap of 5 lets the first case and five candidates run"))
(assign calls 0)
(let [r (check:run five (counted (fn [_] false)) :shrinks 4)]
  (assert (= [false 5] [(r :minimal) calls])
          "and a cap of 4 stops the shrinker after four"))

# A run of k of the 45 elements starts at any of 46 - k places: 1035 runs.
# Each deletion leaves fewer than 45 elements, which passes, so the shrinker
# has more candidates than the default cap.
(def bits-45 @[1 0 1 1 0 1])
(repeat 45 (push bits-45 0))
(assign calls 0)
(let [r (check:run (check:array-of (check:bool) 45)
                   (counted (fn [xs] (< (length xs) 45))) :bits bits-45)]
  (assert (= 45 (length (r :original))) "the bits make 45 elements")
  (assert (= false (r :minimal)) "the default cap stops the shrinker")
  (assert (= 1001 calls) "after the replayed case and 1000 candidates"))

(each [cap why] in [[-1 ":shrinks refuses a negative cap"]
                    [2.5 "and a cap that is not an integer"]]
  (let [err (raises (fn [] (check:run g below-17 :shrinks cap)))]
    (assert (= :argument-error (err :error)) why)
    (assert (string/contains? (err :message) "0 or more")
            (string why ", in a message that names the caps it takes"))))

(let [err (raises (fn [] (check:run g below-17 :sede 3)))]
  (assert (= :argument-error (err :error)) "run refuses an option it lacks")
  (assert (string/contains? (err :message) "sede") "and names it"))

# ── the swap of two neighboring elements ──────────────────────────────
(def digits (check:array-of (check:int 0 9) 6 :min 3))
(defn small? [xs]
  (all? (fn [x] (< x 5)) xs))

(let [r (check:run digits small? :bits [0 0 0 1 0 1 0 0 0 0 0 0 0 0])]
  (assert (= [5 0 0] (r :original)) "the bits make [5 0 0]")
  (assert (= [0 0 5] (r :value)) "and two swaps move the 5 behind the zeros"))

(each seed in (range 5)
  (assert (= [0 0 5] (get (check:run digits small? :depth 0 :seed seed) :value))
          (string "seed " seed ": the zeros go first")))

# An inner array of two bools takes five bits and an empty one two. Neither
# deletion nor lowering reaches the other order, because each passes.
(let [nested (check:array-of (check:array-of (check:bool) 3) 3 :min 2)
      r (check:run nested (fn [xss] (all? (fn [xs] (< (length xs) 2)) xss))
                   :bits [0 1 0 0 0 0 0])]
  (assert (= [[false false] []] (r :original))
          "the bits make a long then a short")
  (assert (= [[] [false false]] (r :value))
          "a swap moves elements of different lengths"))

# ── assert caps as run does ───────────────────────────────────────────
(let [err (raises (fn [] (check:assert g below-17 :bits bits-1000 :shrinks 0)))]
  (assert (= bits-1000 (err :bits)) "assert passes :shrinks to run"))

(let [err (raises (fn [] (check:assert g below-17 :seed 4242)))]
  (assert (string/contains? (err :message) "4242")
          "a seeded failure names its seed")
  (assert (= [0 0 0 0 0 1 0 0 0 1] (err :bits)) "and holds its bits"))

(println "check-shrinks: all tests passed")
