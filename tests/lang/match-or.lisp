(elle/epoch 14)
# audited: 2026-09-30
# An or-pattern matches when any alternative does, every alternative binds the same names, and a failed guard retries the rest.
# docs/match.md

# ── Membership ──────────────────────────────────────────────────────

(defn parity [n]
  "Classify a digit by two or-patterns."
  (match n
    (or 1 3 5 7 9) :odd
    (or 0 2 4 6 8) :even
    _ :out))

(assert (= (parity 1) :odd) "or-pattern: 1 is odd")
(assert (= (parity 9) :odd) "or-pattern: 9 is odd")
(assert (= (parity 0) :even) "or-pattern: 0 is even")
(assert (= (parity 2) :even) "or-pattern: 2 is even")
(assert (= (parity 4) :even) "or-pattern: 4 is even")
(assert (= (parity 42) :out) "or-pattern: no alternative matches 42")

(assert (= (match :b
             (or :a :b :c) :found
             _ :not) :found) "an or-pattern of keywords")
(assert (= (match :y
             (or :x :y) :found
             _ :not) :found) "an or-pattern of two alternatives")
(assert (= (match nil
             (or nil 0) :empty
             _ :other) :empty) "an or-pattern with nil")
(assert (= (match true
             (or true false) :bool) :bool)
        "an or-pattern of both booleans takes any boolean")

# ── Or-patterns inside other patterns ───────────────────────────────

(assert (= (match (pair 2 :x)
             ((or 1 2) . t) t
             _ :fail) :x) "an or-pattern at the head of a pair")
(assert (= (match [2 :x]
             [(or 1 2) y] y
             _ :fail) :x) "an or-pattern inside an array")
(assert (= (match (pair 1 :x)
             (1 . t) t
             (2 . t) t
             ((or 3 4) . t) t
             _ :fail) :x)
        "an or-pattern arm beside plain arms of the same shape")

# ── Bindings ────────────────────────────────────────────────────────

(assert (= (match (pair 1 2)
             (or [x _] (x . _)) x
             _ 0) 1) "the alternative that fits binds the name")
(assert (= (match 99
             (or (x . _) x) x) 99) "the second alternative binds the name")
(let [[ok? _] (protect ((fn ()
                          (eval '(match 1
                                   (or (x . y) (x . _)) :ok
                                   _ :no)))))]
  (assert (not ok?) "alternatives that bind different names fail to compile"))

# ── Guards ──────────────────────────────────────────────────────────

(assert (= (match 2
             (or 1 2 3) when
             true :yes
             _ :no) :yes) "an or-pattern arm whose guard passes")
(assert (= (match 2
             (or 1 2 3) when
             false :never
             _ :fallback) :fallback)
        "an or-pattern arm whose guard fails falls through")
(assert (= (match 5
             (or 1 2 3) when
             true :small
             (or 4 5 6) :medium
             _ :big) :medium)
        "a guarded or-pattern arm gives way to the next or-pattern")
(assert (= (let [threshold 3]
             (match 2
               (or 1 2 3) when
               (< threshold 5) :yes
               _ :no)) :yes) "an or-pattern guard reads an enclosing binding")

# On a guarded arm, a failed guard retries the remaining alternatives of the
# same or-pattern, re-binding and re-testing before the match moves on to the
# next arm. The guard here uses `=`, which may suspend, so the compiler takes
# its sequential path. That is the path that must retry the alternatives.

(assert (= (match (pair :a 5)
             (or (x . _) (_ . x)) when
             (= x 5) x
             _ :none) 5) "a failed guard retries: the second alternative passes")
(assert (= (match (pair 5 :b)
             (or (x . _) (_ . x)) when
             (= x 5) x
             _ :none) 5) "the first alternative passes with no retry")
(assert (= (match (pair 1 2)
             (or (x . _) (_ . x)) when
             (= x 5) x
             _ :none) :none) "no alternative passes, so the arm gives way")
(assert (= (match [1 2 7]
             (or [a _ _] [_ a _] [_ _ a]) when
             (= a 7) a
             _ :none) 7) "of three alternatives, only the last passes")
