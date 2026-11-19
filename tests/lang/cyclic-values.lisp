(elle/epoch 15)
# audited: 2026-11-18
# `=`, `identical?`, `compare` and `hash` each answer on a value that contains itself.
# docs/types.md
#
# The trap: a cycle through a mutable container alone meets the same pair of
# containers again, and a traversal that marks such pairs stops there. A cycle
# that passes through an immutable container on its way meets a pair with one
# mutable side and one immutable side at every step. A traversal that marks
# only pairs of two mutable containers never stops on it, and the program
# overflows its stack instead of answering.

# ── a cycle through mutable containers alone ──────────────────────

(def a @[])
(def b @[])
(push a b)
(push b a)
(assert (= a b) "two @arrays that each hold the other are =")
(assert (identical? a b) "and identical")
(assert (= 0 (compare a b)) "and compare 0")
(assert (= (hash a) (hash b)) "and hash alike")

# ── a cycle through an immutable array ────────────────────────────
# x = @[y] and y = [x]: x against y compares y against x, which compares
# x against y again.

(def x @[])
(def y [x])
(push x y)
(assert (= x y)
        "an @array and the array it holds, each holding the other, are =")
(assert (identical? x y) "and identical")
(assert (= 0 (compare x y)) "and compare 0")
(assert (= (hash x) (hash y)) "and hash alike")

# ── a cycle through an immutable struct ───────────────────────────

(def s @{})
(def t {:k s})
(put s :k t)
(assert (= s t)
        "an @struct and the struct it holds, each holding the other, are =")
(assert (identical? s t) "and identical")
(assert (= 0 (compare s t)) "and compare 0")
(assert (= (hash s) (hash t)) "and hash alike")

# ── a cycle with a difference still answers false ─────────────────

(def p @[1])
(def q [2 p])
(push p q)
(assert (not (= p q)) "a cycle whose first elements differ is not =")
(assert (not (identical? p q)) "and not identical")
(assert (= -1 (compare p q)) "and orders by the first difference")

(println "cyclic-values: all tests passed")
