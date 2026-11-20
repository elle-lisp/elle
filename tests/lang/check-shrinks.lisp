(elle/epoch 18)
# audited: 2026-11-20
# std/check: a run that replays recorded bits, the cap on the candidates the shrinker runs, and the swap of two elements.
# docs/stdlib/check.md
#
# The counter-factual: a shrinker that deletes and lowers alone keeps the
# order it found a case in, so [5 0 0] stays [5 0 0]. A cap that counts
# something other than candidates still stops a long shrink, so the cap tests
# count the property's calls.

(def check ((import "std/check")))

(defn raises [f]
  "The error F raises when called."
  (let [[ok? err] (protect (f))]
    (assert (not ok?) "the call raises")
    err))

(def g (check:int 0 1000))
(defn below-17 [n]
  (< n 17))
(def bits-1000 [1 1 1 1 1 0 1 0 0 0])

# ── :bits replays one case ────────────────────────────────────────────
(let [r (check:run g below-17 :bits bits-1000)]
  (assert (= false (r :passed)) "a replayed case that fails fails the run")
  (assert (= 1000 (r :original)) "the case is the one the bits make")
  (assert (= 17 (r :value)) "and it shrinks")
  (assert (= 1 (r :cases)) "the run tests no other case")
  (assert (= true (r :minimal)) "and the shrinker finishes"))

(let [r (check:run g below-17 :bits bits-1000 :depth 20 :trials 50 :seed 3)]
  (assert (= [1000 1] [(r :original) (r :cases)])
          "depth, trials and seed change nothing"))

(let [r (check:run g below-17 :bits [0 0 0 0 0 0 0 1 0 1])]
  (assert (= true (r :passed)) "a replayed case that passes passes the run")
  (assert (= [1 0] [(r :cases) (r :skipped)]) "as its one case"))

(let [r (check:run g (fn [n] (> n 0)) :bits [])]
  (assert (= [false 0] [(r :passed) (r :value)])
          "a replay past the end of its bits reads zeros"))

(let [r (check:run (check:filter g odd?) (fn [_] false)
                   :bits [0 0 0 0 0 0 0 0 1 0])]
  (assert (= true (r :passed)) "a replayed case that skips passes the run")
  (assert (= [0 1] [(r :cases) (r :skipped)]) "with no case and one skip"))

(let [digits (check:array-of (check:int 0 9) 8)
      sums-under-10 (fn [xs] (< (fold + 0 xs) 10))
      r (check:run digits sums-under-10 :depth 0 :seed 2)
      again (check:run digits sums-under-10 :bits (r :bits))]
  (assert (= (r :value) (again :original) (again :value))
          "a replay of a run's :bits fails on its counterexample")
  (assert (= (r :bits) (again :bits)) "which shrinks no further"))

(each [bits why] in [[[0 2] ":bits refuses a bit that is not 0 or 1"]
                     ["0110" "and a value that is not an array"]]
  (let [err (raises (fn [] (check:run g below-17 :bits bits)))]
    (assert (= :argument-error (err :error)) why)
    (assert (string/contains? (err :message) "0 and 1")
            (string why ", in a message that names the bits it takes"))))

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

# ── assert replays and caps as run does ───────────────────────────────
(assert (= true (check:assert g below-17 :bits [0 0 0 0 0 0 0 1 0 1]))
        "assert passes when the replayed case passes")

(let [err (raises (fn [] (check:assert g below-17 :bits bits-1000)))]
  (assert (= :failed-assertion (err :error))
          "assert fails on a replayed failure")
  (assert (string/contains? (err :message) "replayed bits")
          "and says the case came from replayed bits")
  (assert (= [0 0 0 0 0 1 0 0 0 1] (err :bits))
          "the error holds the bits of the shrunk case"))

(let [err (raises (fn [] (check:assert g below-17 :bits bits-1000 :shrinks 0)))]
  (assert (= bits-1000 (err :bits)) "assert passes :shrinks to run"))

(let [err (raises (fn [] (check:assert g below-17 :seed 4242)))]
  (assert (string/contains? (err :message) "4242")
          "a seeded failure names its seed")
  (assert (= [0 0 0 0 0 1 0 0 0 1] (err :bits)) "and holds its bits"))

(println "check-shrinks: all tests passed")
