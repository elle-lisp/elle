(elle/epoch 18)
# audited: 2026-11-20
# std/check: a run that replays one case from the bits of an earlier run.
# docs/stdlib/check/replay.md
#
# The counter-factual: a run that ran its enumeration in place of the
# replay still fails, and shrinks to the same 17. So each test here names the
# original case the bits make, and how many cases the run tested.

(def check ((import "std/check")))
(include-file "check/helpers.lisp")

(def g (check:int 0 1000))
(defn below-17 [n]
  (< n 17))
(def bits-1000 [1 1 1 1 1 0 1 0 0 0])
(def bits-17 [0 0 0 0 0 1 0 0 0 1])

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

# ── assert replays as run does ────────────────────────────────────────
(assert (= true (check:assert g below-17 :bits [0 0 0 0 0 0 0 1 0 1]))
        "assert passes when the replayed case passes")

(let [err (raises (fn [] (check:assert g below-17 :bits bits-1000)))]
  (assert (= :failed-assertion (err :error))
          "assert fails on a replayed failure")
  (assert (string/contains? (err :message) "replayed bits")
          "and says the case came from replayed bits")
  (assert (= bits-17 (err :bits)) "the error holds the bits of the shrunk case"))

(println "check-replay: all tests passed")
