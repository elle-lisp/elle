(elle/epoch 18)
# audited: 2026-11-20
# std/check: a case that fails in its generator, and a shrinker that keeps each failure in its own place.
# docs/stdlib/check.md
#
# The counter-factual: a generator called outside `protect` raises out of
# `run`, and the counterexample is lost. A shrinker that keeps any failure
# passes a test that asks only for a failing case, so each test here names
# where the shrunk case failed.

(def check ((import "std/check")))

(defn raises-at [bad]
  "An integer from 0 to 1000, from a generator that raises on each BAD one."
  (fn [draw]
    (let [n (draw 1001)]
      (when (bad n)
        (error {:error :bad-draw :n n :message (string "bad draw " n)}))
      n)))

# ── a generator that raises during the shrinking ──────────────────────
# Seed 1 first fails on 580, and lowering it tries 36 on the way to 30.
(let [r (check:run (raises-at (fn [n] (= n 36))) (fn [n] (< n 30)) :depth 0
                   :seed 1)]
  (assert (= false (r :passed)) "a candidate that raises leaves the run whole")
  (assert (= :property (r :failed-in)) "the case fails in the property")
  (assert (= 30 (r :value)) "and shrinks past the candidate that raised")
  (assert (nil? (r :error)) "the shrunk case returned false"))

# ── a generator that raises on a case the search finds ────────────────
(let [r (check:run (raises-at (fn [n] (>= n 500))) (fn [n] true) :depth 0
                   :seed 1)]
  (assert (= false (r :passed)) "a generator that raises fails the run")
  (assert (= :generator (r :failed-in)) "in the generator")
  (assert (nil? (r :value)) "and the case has no value")
  (assert (nil? (r :original)) "the first failing case has none either")
  (assert (= [500] (r :choices)) "it shrinks to the smallest that raises")
  (assert (= [0 1 1 1 1 1 0 1 0 0] (r :bits)) "ten bits for a bound of 1001")
  (assert (= 500 (get (r :error) :n)) ":error is what the shrunk case raised"))

(let [r (check:run (raises-at (fn [n] (= n 2))) (fn [n] true))]
  (assert (= :generator (r :failed-in)) "the enumeration finds it too")
  (assert (= 3 (r :cases)) "on its third case")
  (assert (= [2] (r :choices)) "choice 2"))

(let [r (check:run (fn [draw]
                     (let [a (draw 10)]
                       (when (> a 4) (error {:error :bad-draw :message "a"}))
                       [a (draw 10)])) (fn [_] true) :depth 0 :seed 3)]
  (assert (= :generator (r :failed-in)) "a generator that raises part way")
  (assert (= [5] (r :choices)) "keeps the draws it made before it raised"))

# ── each failure shrinks in its own place ─────────────────────────────
# Every case from 100 up fails, and from 500 up it fails in the generator.
# Seed 1 first fails on 580, so the shrinker passes over 100 to 499.
(let [r (check:run (raises-at (fn [n] (>= n 500))) (fn [n] (< n 100)) :depth 0
                   :seed 1)]
  (assert (= :generator (r :failed-in))
          "a generator failure never shrinks to a property failure")
  (assert (= [500] (r :choices)) "it stops at the smallest that raises"))

# Every case from 40 up fails in the property, and 20 to 29 fail in the
# generator. A shrinker that keeps any failure stops at 20.
(each seed in (range 8)
  (let [r (check:run (raises-at (fn [n] (and (>= n 20) (< n 30))))
                     (fn [n] (< n 40)) :depth 0 :seed seed)]
    (assert (= :property (r :failed-in))
            (string "seed " seed ": a property failure never shrinks onto "
                    "a generator failure"))
    (assert (= 40 (r :value)) "and reaches its smallest")))

# ── assert names what the generator raised ────────────────────────────
(let [[ok? err] (protect (check:assert (raises-at (fn [n] (>= n 500)))
                                       (fn [n] true) :seed 4242))]
  (assert (not ok?) "assert raises when the generator raises")
  (assert (= :failed-assertion (err :error)) "with :failed-assertion")
  (assert (string/contains? (err :message) "bad draw 500")
          "the message names what the shrunk case raised")
  (assert (string/contains? (err :message) "4242") "and the seed"))

(println "check-failures: all tests passed")
