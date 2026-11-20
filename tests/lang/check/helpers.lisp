(elle/epoch 18)
# audited: 2026-11-20
# What the std/check test files share: the error a call raises, and how often a sample holds a value.
# docs/stdlib/check.md
# tests/lang/overview.md

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
