(elle/epoch 10)
# def-shadow: binding resolution for redefined names.
#
# Two fixes:
# 1. File-scope letrec: deferred bindings register AFTER RHS analysis
#    (RHS sees previous binding, not new uninitialized one)
# 2. Explicit letrec: duplicate names rejected at compile time

# ── 1. file-scope: @x then x (same stripped name) ──
# This file itself tests file-scope letrec — the defs below are
# compiled as a synthetic letrec. The second def's RHS must see
# the previous @x binding.
(def @x @[1 2 3])
(def x (freeze x))
(assert (= x [1 2 3]) "file-scope: freeze of @x should produce [1 2 3]")

# file-scope: plain duplicate
(def a 10)
(def a (+ a 1))
(assert (= a 11) "file-scope: (+ a 1) should see previous a=10")

# ── 2. explicit letrec: duplicate names rejected ──
(let [[ok? _] (protect ((fn []
                          (eval '(letrec [x 1
                                   x 2]
                                   x)))))]
  (assert (not ok?) "letrec: duplicate x should be a compile error"))

(let [[ok? _] (protect ((fn []
                          (eval '(letrec [@x @[1 2 3]
                                   x (freeze x)]
                                   x)))))]
  (assert (not ok?) "letrec: @x then x should be a compile error"))
