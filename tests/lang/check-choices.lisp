(elle/epoch 18)
# audited: 2026-11-20
# std/check: a run that replays the case whose draws answer the choices of an earlier run.
# docs/stdlib/check/replay.md
#
# The counter-factual: a replay of choices that clamped a choice past its
# bound to the bound less one answers 1000 for 1001 under a bound of 1001,
# and still makes a case inside the bound. A replay that read on from random
# bits past the end of its choices still makes a value. So each test here
# names the value a choice at or past its bound makes, and the bits the case
# records.

(def check ((import "std/check")))
(include-file "check/parts.lisp")

(include-file "check/helpers.lisp")

(def g (check:int 0 1000))
(defn below-17 [n]
  (< n 17))
(def bits-17 [0 0 0 0 0 1 0 0 0 1])

# ── the source of a replay of choices ─────────────────────────────────
(let [src (source:replay-choices [1 2 3 9])
      draw (src :draw)
      answers [(draw 3) (draw 4) (draw 3) (draw 5) (draw 7)]
      record ((src :record))]
  (assert (= [1 2 0 0 0] answers)
          "each draw answers its choice, 0 at or past its bound or the end")
  (assert (= [1 2 0 0 0] (record :choices)) "and records what it answered")
  (assert (= [0 1 1 0 0 0 0 0 0 0 0 0] (record :bits))
          "in the bits of each draw's width")
  (assert (= [0 2 4 6 9] (record :starts)) "where each draw's bits start")
  (assert (= (record :starts) (record :accepted)) "and no draw rejects bits")
  (assert (= [2 4 6 9 12] (record :ends)) "where they end"))

(assert (= 3 (((source:replay-choices [3]) :draw) 10 [9]))
        "a replay of choices ignores edges")

# ── :choices replays one case ─────────────────────────────────────────
(let [r (check:run g below-17 :choices [1000])]
  (assert (= false (r :passed)) "a case the choices make that fails fails")
  (assert (= 1000 (r :original)) "the case is the one the choices make")
  (assert (= [17 [17] bits-17] [(r :value) (r :choices) (r :bits)])
          "and it shrinks")
  (assert (= [1 true] [(r :cases) (r :minimal)])
          "the run tests no other case, and the shrinker finishes"))

(let [r (check:run g below-17 :choices [1000] :depth 20 :trials 50 :seed 3)]
  (assert (= [1000 1] [(r :original) (r :cases)])
          "depth, trials and seed change nothing"))

(let [r (check:run g below-17 :choices [3])]
  (assert (= true (r :passed)) "a case the choices make that passes passes")
  (assert (= [1 0] [(r :cases) (r :skipped)]) "as its one case"))

(each choices in [[1001] [5000] []]
  (let [r (check:run g (fn [n] (> n 0)) :choices choices)]
    (assert (= [false 0] [(r :passed) (r :original)])
            (string "the choices " choices " answer 0"))))

(let [r (check:run (check:tuple (check:int 0 9) (check:int 0 9)) (fn [_] false)
                   :choices [7] :shrinks 0)]
  (assert (= [7 0] (r :original)) "a draw past the end of the choices answers 0")
  (assert (= [0 1 1 1 0 0 0 0] (r :bits)) "and records the bits of 0"))

(let [five (fn [draw] (draw 5))
      below-3 (fn [n] (< n 3))
      rejected (check:run five below-3 :bits [1 1 1 0 1 1] :shrinks 0)
      chosen (check:run five below-3 :choices (rejected :choices) :shrinks 0)]
  (assert (= [3 3] [(rejected :original) (chosen :original)])
          "a replay of a case's choices makes the value of its bits")
  (assert (= [0 1 1] (chosen :bits)) "with none of the bits it rejected"))

(let [digits (check:array-of (check:int 0 9) 8)
      never (fn [_] false)]
  (assert (= [5 7]
             (get (check:run digits never :choices [2 5 7] :shrinks 0) :original))
          "a repeated draw takes its length, then each element, as choices")
  (assert (= []
             (get (check:run digits never :choices [9 5 7] :shrinks 0) :original))
          "and a length at its bound answers 0"))

(let [r (check:run (check:filter g odd?) (fn [_] false) :choices [0])]
  (assert (= true (r :passed)) "a case the choices make that skips passes")
  (assert (= [0 1] [(r :cases) (r :skipped)]) "with no case and one skip"))

(let [pair (check:tuple (check:function (check:int 0 100)) (check:int 0 100))
      under-50 (fn [[f x]] (< (f x) 50))
      r (check:run pair under-50 :choices [0 50] :shrinks 0)]
  (assert (= [[[[0] 50]]] (r :calls)) "a call draws its answer from the choices")
  (assert (= 0 ((get (r :value) 0) 7))
          "and a call after the case answers the simplest value"))

(defn big-pair? [[a b]]
  (and (> a 500) (> b 500)))
(each seed in (range 5)
  (let [pairs (check:array-of (check:tuple g g) 6)
        prop (fn [xs] (not (any? big-pair? xs)))
        r (check:run pairs prop :depth 0 :seed seed)
        again (check:run pairs prop :choices (r :choices))]
    (assert (= (r :value) (again :original) (again :value))
            (string "seed " seed
                    ": a replay of a run's :choices fails on its counterexample"))
    (assert (= (r :bits) (again :bits)) "whose bits it makes")))

(each [choices why] in [[[-1] ":choices refuses a negative choice"]
                        [[1.5] "and a choice that is not an integer"]
                        ["01" "and a value that is not an array"]]
  (let [err (raises (fn [] (check:run g below-17 :choices choices)))]
    (assert (= :argument-error (err :error)) why)
    (assert (string/contains? (err :message) "0 or more")
            (string why ", in a message that names the choices it takes"))))

(let [err (raises (fn [] (check:run g below-17 :bits [0] :choices [0])))]
  (assert (= :argument-error (err :error)) "run refuses :bits with :choices")
  (assert (and (string/contains? (err :message) ":bits")
               (string/contains? (err :message) ":choices"))
          "in a message that names both"))

# ── assert replays as run does ────────────────────────────────────────
(assert (= true (check:assert g below-17 :choices [3]))
        "assert passes when the case the choices make passes")

(let [err (raises (fn [] (check:assert g below-17 :choices [1000])))]
  (assert (= :failed-assertion (err :error))
          "assert fails on a case the choices make")
  (assert (string/contains? (err :message) "replayed choices")
          "and says the case came from replayed choices")
  (assert (= bits-17 (err :bits)) "the error holds the bits of the shrunk case"))

(println "check-choices: all tests passed")
