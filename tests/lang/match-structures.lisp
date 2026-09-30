(elle/epoch 14)
# audited: 2026-09-30
# match patterns take arrays, pairs, lists and structs apart, to any depth, and bind their parts.
# docs/match.md

# ── Arrays ──────────────────────────────────────────────────────────

(assert (= (match [1 2 3]
             [1 2 3] "exact"
             _ "no") "exact") "an array of literals matches an equal array")
(assert (= (match [10 20]
             [a b] (+ a b)
             _ 0) 30) "an array pattern binds each element")
(assert (= (match [1 2]
             [a b c] "three"
             [a b] "two"
             _ nil) "two") "an array pattern of another length does not match")
(assert (= (match 42
             [a b] "array"
             _ "other") "other") "an array pattern does not match a non-array")
(assert (= (match []
             [] "empty"
             _ "other") "empty")
        "the empty array pattern matches an empty array")
(assert (= (match [1 2 3 4]
             [a & rest] (length rest)
             _ 0) 3) "an array rest pattern binds the remaining elements")
(assert (= (match [1 [2 3]]
             [a [b c]] (+ a (+ b c))
             _ 0) 6) "array patterns nest")
(assert (= (match [1 [2 3]]
             [1 [2 3]] :exact
             [1 [2 _]] :partial
             [_ _] :any-pair
             _ :other) :exact)
        "the most specific nested array arm listed first wins")

# ── Pairs and lists ─────────────────────────────────────────────────

(assert (= (match (pair 1 2)
             (h . t) (+ h t)
             _ 0) 3) "a pair pattern binds the head and the tail")
(assert (= (match 42
             (h . t) "pair"
             _ "nope") "nope") "a pair pattern does not match a non-pair")
(assert (= (match (list 1 2)
             nil :nil
             (h . t) :pair
             _ :other) :pair) "a non-empty list is a pair")
(assert (= (match (list 1 2 3)
             (a & rest) a
             _ nil) 1) "a list rest pattern binds the first element")
(assert (= (match (list 1 2 3)
             (1 2) "two"
             (1 2 3) "three"
             _ nil) "three") "a list pattern matches only its own length")
(assert (= (match (list 1 2 3)
             (1 2 3) :exact
             (1 2 _) :prefix
             _ :other) :exact)
        "of two arms sharing a prefix, the exact one answers")
(assert (= (match (list 1 2 4)
             (1 2 3) :exact
             (1 2 _) :prefix
             _ :other) :prefix)
        "of two arms sharing a prefix, the one that fits answers")
(assert (= (match (list 1 (list 2 (list 3)))
             (1 (2 (3))) :deep
             (1 (2 _)) :medium
             (1 _) :shallow
             _ :none) :deep) "list patterns nest")

# ── Improper list patterns ──────────────────────────────────────────

(assert (= (match (pair 1 (pair 2 3))
             (a b . c) (list a b c)
             _ :no) (list 1 2 3)) "an improper list pattern binds its tail")
(assert (= (match (list 1 2 3 4 5)
             (a b c . d) (list a b c d)
             _ :no) (list 1 2 3 (list 4 5)))
        "an improper list pattern binds the rest of a longer list")
(assert (= (match (pair 1 2)
             (a . b) (list a b)
             _ :no) (list 1 2)) "a one-element improper pattern matches a pair")
(assert (= (match (list 1)
             (a b . c) :matched
             _ :no) :no)
        "an improper list pattern does not match a shorter list")

# ── Structs ─────────────────────────────────────────────────────────

(assert (= (match {:type :circle :radius 5}
             {:type :circle :radius r} r
             {:type :square :side s} s
             _ 0) 5)
        "a struct pattern matches on a literal value and binds another key")
(assert (= (match {:type :square :side 7}
             {:type :circle :radius r} r
             {:type :square :side s} s
             _ 0) 7) "a struct arm whose literal differs gives way to the next")
