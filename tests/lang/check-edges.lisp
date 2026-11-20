(elle/epoch 18)
# audited: 2026-11-20
# std/check: the edges a draw names, which one random draw in 8 answers, and the bits a case records for them.
# docs/stdlib/check.md
#
# The counter-factual: a uniform draw over 2^41 values meets neither bound in
# a thousand cases, so a property that fails at a bound alone passes. A random
# source that answered an edge without recording its bits would make a case
# that its replay cannot repeat.

(def check ((import "std/check")))
(def source ((import "std/check/source")))

(defn raises [f]
  "The error F raises when called."
  (let [[ok? err] (protect (f))]
    (assert (not ok?) "the call raises")
    err))

(defn hits? [xs v]
  "Whether V is among XS."
  (any? (fn [x] (= x v)) xs))

(defn count-of [xs v]
  "How many of XS are V."
  (fold (fn [n x] (if (= x v) (+ n 1) n)) 0 xs))

(def big (bit/shl 1 40))

# ── check:int names lo, 0 and hi ──────────────────────────────────────
(let [xs (check:sample (check:int (- big) big) :count 400)]
  (assert (hits? xs (- big)) "a random int over 0 reaches lo")
  (assert (hits? xs 0) "and 0")
  (assert (hits? xs big) "and hi"))

(let [xs (check:sample (check:int -3 big) :count 400)]
  (assert (and (hits? xs -3) (hits? xs big))
          "a range longer above 0 reaches both ends"))
(let [xs (check:sample (check:int (- big) 3) :count 400)]
  (assert (and (hits? xs (- big)) (hits? xs 3)) "and a range longer below 0"))
(let [xs (check:sample (check:int 7 (+ 7 big)) :count 400)]
  (assert (and (hits? xs 7) (hits? xs (+ 7 big)))
          "a range above 0 reaches both ends"))
(let [xs (check:sample (check:int (- -7 big) -7) :count 400)]
  (assert (and (hits? xs (- -7 big)) (hits? xs -7)) "and a range below 0"))

(let [g (check:int (- big) big)
      r (check:run g (fn [n] (< n big)) :trials 400)]
  (assert (= false (r :passed)) "a property that fails at hi alone fails")
  (assert (= big (r :value)) "on hi")
  (assert (= big (g ((source:replay (r :bits)) :draw)))
          "and a replay of its bits makes hi"))

# ── how often a random draw answers an edge ───────────────────────────
(let [xs (check:sample (fn [draw] (draw big [7])) :seed 1 :count 800)
      n (count-of xs 7)]
  (assert (and (<= 60 n) (<= n 140))
          (string "one draw in 8 answers its edge: " n " of 800")))

(let [xs (check:sample (fn [draw] (draw big [5 9])) :seed 2 :count 1600)]
  (each v in [5 9]
    (let [n (count-of xs v)]
      (assert (and (<= 60 n) (<= n 140))
              (string "each edge as often as the other: " n " of 1600 are " v)))))

# ── the bits of a random case replay it ───────────────────────────────
(let [g (fn [draw] [(draw big [(- big 1)]) (draw 1001 [1000 3])])
      next-source (source:random 3)
      @edges 0]
  (repeat 300
    (let [src (next-source)
          v (g (src :draw))
          bits (((src :record)) :bits)]
      (when (= (- big 1) (get v 0)) (assign edges (+ edges 1)))
      (assert (= v (g ((source:replay bits) :draw)))
              "a replay of a random case's bits makes its value")))
  (assert (< 0 edges) "and some of those cases answered an edge"))

# ── the enumeration and a replay ignore edges ─────────────────────────
(assert (= [0 1 2 3] (check:enumerate (fn [draw] (draw 10 [9])) 3))
        "the enumeration ignores edges")
(assert (= 3 (((source:replay [0 0 1 1]) :draw) 10 [9])) "and so does a replay")

# ── a draw that names no edges ────────────────────────────────────────
# The values a SplitMix64 stream from seed 7 names for bounds of 1000. A
# source that read bits to decide on every draw would answer others.
(def plain [399 190 121 89 200 221])
(assert (= plain (check:sample (fn [draw] (draw 1000)) :seed 7 :count 6))
        "a draw that names no edges reads no bits to decide")
(assert (= plain (check:sample (fn [draw] (draw 1000 [])) :seed 7 :count 6))
        "and neither does a draw that names an empty array")

# ── refused edges ─────────────────────────────────────────────────────
(def any-draw ((source:replay []) :draw))
(each [edges why] in [[[10] "a draw refuses an edge past its bound"]
                      [[-1] "and a negative edge"]
                      [[2.5] "and an edge that is not an integer"]
                      [9 "and edges that are not an array"]]
  (let [err (raises (fn [] (any-draw 10 edges)))]
    (assert (= :argument-error (err :error)) why)
    (assert (string/contains? (err :message) "edges")
            (string why ", in a message that names the edges"))))

(println "check-edges: all tests passed")
