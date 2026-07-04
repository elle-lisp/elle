(elle/epoch 12)
# `=` is compositional: element equality agrees with the equality of
# single-element collections, for all a, b:  (= [a] [b])  <=>  (= a b).
#
# Spec (docs/types.md § Equality): numeric coercion and IEEE 754 float
# semantics apply at EVERY depth. (= 1 1.0) and (= [1] [1.0]) are both
# true; NaN is never `=` to anything — even inside collections, even
# when comparing a NaN-holding value to itself (no identity shortcut).
#
# HISTORY: until epoch 12 `src/arithmetic.rs` applied numeric coercion /
# NaN handling only to the TOP-LEVEL pair; collection elements fell
# through to structural PartialEq, so (= [1] [1.0]) was false and
# (= [nan] [nan]) was true. The JIT helper additionally disagreed at
# top level (bitwise NaN fast path, int-int through f64).
#
# Constraint pinned below: key equivalence (sets, struct keys, hash)
# stays NaN-reflexive and number-coercive, or a collection containing
# itself-unequal elements could not be found in a set that holds it.

(def nan (/ 0.0 0.0))

# ── The invariant itself, fix-direction-agnostic ────────────────────
(assert (= (= 1 1.0) (= [1] [1.0]))
        "(= [1] [1.0]) must equal (= 1 1.0): equality composes through arrays")
(assert (= (= nan nan) (= [nan] [nan]))
        "(= [n] [n]) must equal (= n n) for NaN: equality composes through arrays")

# ── Coercion facet: 1 vs 1.0 at depth, across container kinds ───────
(assert (= [1] [1.0]) "array elements coerce: (= [1] [1.0])")
(assert (= [[1]] [[1.0]]) "coercion reaches depth 2")
(assert (= '(1) '(1.0)) "list elements coerce")
(assert (= {:a 1} {:a 1.0}) "struct values coerce")
(assert (= {:a [1]} {:a [1.0]}) "coercion reaches nested struct values")
(assert (= (set 1) (set 1.0)) "set elements coerce")
(assert (= (box 1) (box 1.0)) "box contents coerce")
(assert (= @[1] [1.0]) "coercion composes with the mutability boundary")
(assert (= [1] [1.0] @[1]) "chained = composes through collections")
(assert (not (= [1] [2.0])) "unequal numbers stay unequal inside collections")

# ── IEEE float facet: NaN poisons, zeros unify, at depth ────────────
(assert (not (= nan nan)) "IEEE 754: (= nan nan) is false")
(assert (not (= [nan] [nan])) "NaN stays unequal inside arrays")
(assert (not (= {:a nan} {:a nan})) "NaN stays unequal inside struct values")
(assert (not (= (list :t nan) (list :t nan))) "NaN stays unequal inside lists")
(def v [nan])
(assert (not (= v v))
        "no identity shortcut: a NaN-holding value is not = to itself")
(assert (= -0.0 0.0) "IEEE 754: zeros are equal")
(assert (= [-0.0] [0.0]) "zero equality composes through arrays")

# ── Exactness facet: int-int never coerces through f64 ──────────────
(assert (not (= 9007199254740992 9007199254740993))
        "int-int comparison is exact beyond 2^53")
(assert (not (= [9007199254740992] [9007199254740993]))
        "int-int exactness composes through arrays")

# ── identical? is the strict relation, unchanged by composition ─────
(assert (not (identical? 1 1.0)) "identical? does not coerce")
(assert (not (identical? [1] [1.0]))
        "identical? does not coerce inside collections")
(assert (identical? [nan] [nan])
        "identical? floats compare by bit pattern (reflexive)")

# ── Key equivalence: coercive, NaN-reflexive — findability holds ────
(assert (= (length (set 1 1.0)) 1) "key equivalence dedups 1 and 1.0")
(assert (has? (set 1.0) 1) "set membership coerces numbers")
(assert (has? (set nan) nan) "NaN is findable in a set that holds it")
(assert (has? (set [nan]) [nan])
        "a NaN-holding collection is findable in a set that holds it")

(println "equality-composition: OK")
