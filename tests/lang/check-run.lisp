(elle/epoch 18)
# audited: 2026-11-20
# std/check: the cases `run` tests, the verdict it returns, and the counterexample it shrinks to.
# docs/stdlib/check.md
#
# The counter-factual: a shrinker that stops at the first smaller failure
# passes a test that asks only for "a smaller case". Each shrinking test here
# picks a property with one smallest failure, and names that case.
#
# The trap: a case read most significant bit first cannot drop its trailing
# zero bits, because 512 of a 10-bit draw would shorten to one bit and rank
# simpler than 1. Each `:bits` below is every bit of every draw.

(def check ((import "std/check")))

# ── run: a property that holds ────────────────────────────────────────
(let [ok (check:run (check:int 0 9) (fn [n] (< n 10)) :trials 5)]
  (assert (= true (ok :passed)) "a property that holds passes")
  (assert (= 10 (ok :cases)) "5 cases at the default depth of 4, and 5 trials")
  (assert (nil? (get ok :value)) "a run that passed holds no counterexample"))

(assert (= 104
           (get (check:run (check:int 0 1000) (fn [n] (<= 0 n)) :depth 3) :cases))
        "the default is 100 trials")
(assert (= 105 (get (check:run (check:int 0 1000) (fn [n] (<= 0 n))) :cases))
        "and a depth of 4")
(assert (= true (get (check:run (check:int 0 9) (fn [n] n)) :passed))
        "a property passes on any value but false and nil, 0 included")
(assert (= true
           (get (check:run (check:int 0 1000) (fn [n] (< n 17)) :depth 4
                           :trials 0) :passed))
        "a run with no trials tests the enumerated cases alone")

# ── run: a property that fails, and the case it shrinks to ────────────
(let [r (check:run (check:int 0 1000) (fn [n] (< n 17)))]
  (assert (= false (r :passed)) "a property that returns false fails")
  (assert (= 17 (r :value)) "shrinking finds the smallest failure")
  (assert (= [17] (r :choices)) ":choices holds the choice of each draw")
  (assert (= [0 0 0 0 0 1 0 0 0 1] (r :bits))
          ":bits holds the bits of each draw, ten for a bound of 1001")
  (assert (>= (r :original) 17) ":original is a failing case")
  (assert (nil? (r :error)) "a case that returned false raised nothing"))

(let [r (check:run (check:int 0 1000) (fn [n] (< n 17)) :depth 20 :trials 0)]
  (assert (= 17 (r :original)) "the enumeration reaches the failure first")
  (assert (= 18 (r :cases)) "after 17 cases that passed")
  (assert (= 17 (r :value)) "and it is already the smallest"))

(let [r (check:run (check:int 0 1000) (fn [n] (if (< n 17) true nil)))]
  (assert (= false (r :passed)) "a property that returns nil fails")
  (assert (= 17 (r :value)) "and shrinks the same way"))

(let [r (check:run (check:int 0 1000)
                   (fn [n]
                     (when (> n 20)
                       (error {:error :boom :n n :message "past 20"}))
                     true))]
  (assert (= false (r :passed)) "a property that raises fails")
  (assert (= 21 (r :value)) "and shrinks to the smallest case that raises")
  (assert (= :boom (get (r :error) :error)) ":error holds what the case raised")
  (assert (= 21 (get (r :error) :n)) "from the shrunk case, not the first one"))

(let [r (check:run (check:int 0 9) (fn [n] false))]
  (assert (= 0 (r :value))
          "a property that always fails shrinks to the simplest case")
  (assert (= [0] (r :choices)) "choice 0 of its one draw")
  (assert (= [0 0 0 0] (r :bits)) "read as the four bits of a bound of 10")
  (assert (= 1 (r :cases)) "and it failed on the first case"))

(let [r (check:run (check:tuple (check:int 0 100) (check:int 0 100))
                   (fn [[a b]] (or (< a 10) (< b 20))))]
  (assert (= [10 20] (r :value)) "each part of a tuple shrinks")
  (assert (= [10 20] (r :choices)) "to the choices that make it")
  (assert (= [0 0 0 1 0 1 0 0 0 1 0 1 0 0] (r :bits))
          "seven bits for each bound of 101"))

(let [r (check:run (check:array-of (check:int 0 9) 8)
                   (fn [xs] (< (length xs) 3)))]
  (assert (= [0 0 0] (r :value))
          "an array shrinks to the shortest failure, of the simplest elements")
  (assert (= [3 0 0 0] (r :choices))
          "one draw for the length, and one for each element")
  (assert (= [0 0 1 1 0 0 0 0 0 0 0 0 0 0 0 0] (r :bits))
          "four bits each: a bound of 9 for the length, and of 10 for a digit"))

(let [prop (fn [xs] (< (fold + 0 xs) 10))
      r (check:run (check:array-of (check:int 0 9) 8) prop)]
  (assert (= false (r :passed)) "some array of digits sums to 10 or more")
  (assert (not (prop (r :value))) "the shrunk case still fails")
  (assert (= 10 (fold + 0 (r :value)))
          "and lowering any element of it by 1 would pass"))

(let [r (check:run (check:map (fn [n] (* 2 n)) (check:int 0 1000))
                   (fn [n] (< n 51)))]
  (assert (= 52 (r :value)) "a mapped generator shrinks through its choices")
  (assert (= [26] (r :choices)) "which are those of the generator it wraps")
  (assert (= [0 0 0 0 0 1 1 0 1 0] (r :bits)) "and so are its bits"))

# ── run: the bits of a shrunk case ────────────────────────────────────
(let [r (check:run (check:int 0 1000) (fn [n] (< n 512)) :depth 0)]
  (assert (= 512 (r :value)) "a random failure shrinks to the smallest")
  (assert (= [1 0 0 0 0 0 0 0 0 0] (r :bits))
          "and its bits keep their trailing zeros"))

# A bound of 5 reads three bits, and rejects the bits of 5, 6 and 7. Over
# twenty seeds some first failure rejects bits, and its shrunk case keeps none.
# An edge records no rejected bits, so the draw names none.
(each seed in (range 20)
  (let [r (check:run (fn [draw] (draw 5)) (fn [n] (< n 3)) :depth 0 :seed seed)]
    (assert (= 3 (r :value)) (string "seed " seed " shrinks to 3"))
    (assert (= [0 1 1] (r :bits))
            (string "seed " seed ": a shrunk case keeps no rejected bits"))))

(let [source ((import "std/check/source"))
      g (check:array-of (check:int 0 9) 8)
      r (check:run g (fn [xs] (< (fold + 0 xs) 10)))]
  (assert (= (r :value) (g ((source:replay (r :bits)) :draw)))
          "a replay of :bits makes :value"))

# ── run: the seed decides the random cases ────────────────────────────
# Two seeds can both fail first on an edge, so the draw names none.
(let [g (fn [draw] (draw 1000001))
      fails (fn [n] (< n 17))
      a (check:run g fails :depth 0 :seed 11)
      b (check:run g fails :depth 0 :seed 11)
      c (check:run g fails :depth 0 :seed 12)]
  (assert (= (a :original) (b :original))
          "one seed fails on the same case twice")
  (assert (not= (a :original) (c :original)) "and another seed on another case")
  (assert (= 17 (a :value) (c :value)) "both shrink to the one smallest failure"))

# ── assert ────────────────────────────────────────────────────────────
(assert (= true
           (check:assert (check:array-of
                           (check:tuple (check:int 0 9) (check:int 0 9)) 8)
                         (fn [xs] (= (sort (sort xs)) (sort xs)))))
        "assert returns true when the property holds")

(let [[ok? err] (protect (check:assert (check:int 0 99) (fn [n] (< n 50))
                                       :seed 4242))]
  (assert (not ok?) "assert raises when the property fails")
  (assert (= :failed-assertion (err :error)) "with :failed-assertion")
  (assert (string/contains? (err :message) "50")
          "the message names the shrunk counterexample")
  (assert (string/contains? (err :message) "4242") "and the seed"))

(println "check-run: all tests passed")
