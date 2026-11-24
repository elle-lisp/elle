(elle/epoch 19)
# audited: 2026-11-24
# match tries its arms in order and answers the body of the first whose pattern fits: literals, wildcards and bindings.
# docs/match.md
#
# match-structures.lisp covers arrays, lists, pairs and structs, match-guards.lisp
# the guards, match-or.lisp the or-patterns, and match-reachability.lisp the
# arms the compiler rejects and the values no arm takes.

# ── Literals match by equality ──────────────────────────────────────

(assert (= (match 5
             5 "five"
             _ nil) "five") "an int literal matches an equal int")
(assert (= (match "hello"
             "hello" "matched"
             _ "no") "matched") "a string literal matches an equal string")
(assert (= (match :foo
             :foo "matched"
             _ "no") "matched") "a keyword literal matches the same keyword")
(assert (= (match :bar
             :foo "matched"
             _ "no") "no") "a keyword literal does not match another keyword")
(assert (= (match nil
             nil "empty"
             _ nil) "empty") "the nil pattern matches nil")
(assert (= (match (list)
             nil "empty"
             _ "not-nil") "not-nil")
        "the nil pattern does not match the empty list")
(assert (= (match true
             true :t
             false :f) :t) "a boolean literal matches true")
(assert (= (match false
             true :yes
             false :no) :no) "a boolean literal matches false")
(assert (= (match :c
             :a 1
             :b 2
             :c 3
             :d 4
             _ 0) 3) "among many literal arms, the equal one answers")

# ── A quoted symbol matches that symbol ─────────────────────────────

(assert (= (match 'x
             'x :x
             _ :other) :x) "a quoted symbol matches the same symbol")
(assert (= (match 'y
             'x :x
             _ :other) :other) "a quoted symbol does not match another symbol")
(assert (= (match "x"
             'x :x
             _ :other) :other) "a quoted symbol does not match a string")
(assert (= (match :x
             'x :x
             _ :other) :other) "a quoted symbol does not match a keyword")
(assert (= (match '(if a b c)
             ('if c t e) [c t e]
             _ :other) ['a 'b 'c])
        "a quoted symbol matches a list's head, and the names bind the rest")
(assert (= (match 'b
             (or 'a 'b) :ab
             _ :other) :ab) "a quoted symbol is an or-pattern alternative")

(defn compiles? [src]
  (first (protect (compile/whole-module src "<matching>"))))
(assert (not (compiles? "(defn f [s] (match s 'a 1 'a 2 _ 3))"))
        "a second arm for the same quoted symbol is unreachable")
(assert (compiles? "(defn f [s] (match s 'a 1 'b 2 _ 3))")
        "arms for two quoted symbols are both reachable")

# ── Arms are tried in order ─────────────────────────────────────────

(assert (= (match 5
             5 :first
             x :second) :first) "an earlier arm that fits wins over a later one")
(assert (= (match 1
             1 "one"
             2 "two"
             3 "three"
             _ nil) "one") "the first arm answers its value")
(assert (= (match 2
             1 "one"
             2 "two"
             3 "three"
             _ nil) "two")
        "a later arm answers when the earlier ones do not fit")
(assert (= (match 99
             1 "one"
             2 "two"
             _ "other") "other") "the wildcard answers when no literal fits")

# ── Wildcards and bindings ──────────────────────────────────────────

(assert (= (match 42
             _ :caught) :caught) "the wildcard catches a positive int")
(assert (= (match -1000
             _ :caught) :caught) "the wildcard catches a negative int")
(assert (= (match 0
             _ :caught) :caught) "the wildcard catches zero")
(assert (= (match "hello"
             _ "matched") "matched") "the wildcard catches a string")
(assert (= (match 42
             x (+ x 1)) 43) "a bare symbol binds the value")
(assert (= (match 42
             1 :one
             x x) 42) "a bare symbol is a catch-all")

# ── The chosen body is an expression ────────────────────────────────

(assert (= (match 10
             10 (* 2 3)
             _ nil) 6) "the chosen arm's body is evaluated")
(assert (= (+ 1
              (match 42
                42 42
                _ 0)) 43) "match answers a value in argument position")
(assert (= (+ 1
              (match 7
                7 7
                _ 0)) 8)
        "match answers a value in argument position: another value")

# A match inside a loop body answers once per pass. The accumulator is a
# top-level mutable reassigned on each pass, the shape
# tests/impl/region-toplevel-mutable-reassign.lisp pins.
(def @test-result (list))
(each i (list 1 2 3)
  (assign
    test-result
    (pair (match i
            1 :one
            2 :two
            3 :three
            _ :other) test-result)))
(assert (= (reverse test-result) (list :one :two :three))
        "a match in a loop answers on every pass")
