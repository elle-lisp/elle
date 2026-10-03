(elle/epoch 14)
# audited: 2026-09-30
# A guard is a condition an arm must pass as well as its pattern; a failed guard moves the match on to the next arm.
# docs/match.md
#
# match-or.lisp covers a guard on an or-pattern, which retries the remaining
# alternatives before it moves on.

# ── A guard sees the pattern's bindings ─────────────────────────────

(assert (= (match 5
             x when
             (> x 0) :pos
             x when
             (< x 0) :neg
             _ :zero) :pos) "a guard sees its binding: positive")
(assert (= (match -3
             x when
             (> x 0) :pos
             x when
             (< x 0) :neg
             _ :zero) :neg) "a guard sees its binding: negative")
(assert (= (match 0
             x when
             (> x 0) :pos
             x when
             (< x 0) :neg
             _ :zero) :zero) "a guard sees its binding: neither guard passes")
(assert (= (match 5
             x when
             (> x 3) :big
             x :middle) :big) "an arm whose guard passes answers")
(assert (= (match 2
             x when
             (> x 3) :big
             x :small) :small) "an arm whose guard fails gives way to the next")
(assert (= (match 5
             x when
             (> x 10) :big
             x when
             (> x 3) :medium
             x :small) :medium) "the first arm whose guard passes answers")

# ── A guard on each pattern shape ───────────────────────────────────

(assert (= (match 10
             10 when
             false "nope"
             10 "yes"
             _ nil) "yes") "a guard on a literal arm")
(assert (= (match (pair 1 2)
             (h . t) when
             (> h 0) (+ h t)
             _ 0) 3) "a guard on a pair pattern")
(assert (= (match (list 1 2 3)
             (a b c) when
             (> (+ a b c) 5) :big
             _ :small) :big) "a guard on a list pattern")
(assert (= (match [1 2]
             [a b] when
             (< a b) :ordered
             _ :no) :ordered) "a guard on an array pattern")
(assert (= (match {:x 10 :y 20}
             {:x x :y y} when
             (> y x) :valid
             _ :no) :valid) "a guard on a struct pattern")
(assert (= (match (list 1 2 3)
             (a & rest) when
             (> a 0) rest
             _ :fail) (list 2 3)) "a guard on a rest pattern")

# ── A failed guard falls through ────────────────────────────────────

(assert (= (match 5
             x when
             false :never
             x :always) :always) "a failed guard falls through to a binding arm")
(assert (= (match 5
             x when
             false :a
             _ :fallback) :fallback)
        "a failed guard falls through to the wildcard")
(assert (= (match 5
             5 when
             false :guarded
             5 :unguarded
             _ :default) :unguarded)
        "a failed guard falls through to an unguarded arm with the same literal")
(assert (= (match 5
             x when
             false x
             y (+ y 1)) 6)
        "a failed guard's binding does not reach the next arm")

# ── The body runs after the guard passes ────────────────────────────

(assert (= (match 10
             x when
             (> x 5)
               (let [y (* x 2)]
                 y)
             x x) 20) "a guarded body is any expression")
