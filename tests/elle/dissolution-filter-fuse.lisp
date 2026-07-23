(elle/epoch 12)
# Filter loop fusion — value preservation + realization (docs/impl/dissolution.md).
#
# `(filter p xs)` over a proven immutable array with an inline non-capturing
# predicate dissolves to an inlined index-walk loop with a GUARDED push: the
# element is bound once and pushed only when `(p item)` is truthy — no per-element
# closure, no `filter` dispatch. A `(filter q (filter p xs))` fuses to one loop
# with the guards nested (no intermediate array). This file is the behavioral
# gauge: the fused form must compute EXACTLY what the un-fused `filter` computes.
# The codegen gauge (that the closure and dispatch are gone and the push is
# guarded by an `if`) lives in `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference is `filter` applied to a NAMED predicate (`big?`) — a
# Var, not a lambda literal, so the gate leaves it a plain `filter` call. Fused
# inline-predicate and un-fused named-pred must agree.

(defn big? [x]
  (> x 2))

# Single filter: fused inline predicate == un-fused named pred == literal expect.
(assert (= (filter (fn [x] (> x 2)) [1 2 3 4]) [3 4])
        "single filter fuses to the same value")
(assert (= (filter (fn [x] (> x 2)) [1 2 3 4]) (filter big? [1 2 3 4]))
        "fused inline-predicate agrees with un-fused named-pred")

# Boundary sizes and the all/none-pass extremes.
(assert (= (filter (fn [x] (> x 0)) []) []) "empty array filters to empty")
(assert (= (filter (fn [x] (> x 0)) [7]) [7]) "singleton kept")
(assert (= (filter (fn [x] (> x 9)) [7]) []) "singleton dropped")
(assert (= (filter (fn [x] (> x 0)) [1 2 3]) [1 2 3]) "all kept")
(assert (= (filter (fn [x] (> x 9)) [1 2 3]) []) "none kept")

# A predicate that uses its parameter more than once — the element must be
# evaluated once and bound, not re-substituted (the loop binds `item` once).
(assert (= (filter (fn [x] (> (* x x) 4)) [1 2 3 4]) [3 4])
        "multi-use parameter in the predicate fuses correctly")

# The base collection reached through a Var alias fuses just as a call-site
# literal does (the base-alias proof and the guarded-push shape compose).
(assert (= (let [xs [1 2 3 4]]
             (filter (fn [x] (> x 2)) xs)) [3 4])
        "filter over a Var-bound immutable array fuses to the same value")

# Composition fuses to ONE loop; the survivor set/order is unchanged and the
# interleaving of the two reorder-safe predicates is unobservable. `integer?` and
# `even?` are reorder-safe (they carry only SIG_ERROR); a variadic comparison like
# `>` routes through `apply` and would decline the composition (still fuses as a
# single filter).
(assert (= (filter (fn [x] (even? x))
                   (filter (fn [x] (integer? x)) [1 2 3 4 5 6])) [2 4 6])
        "filter-of-filter fuses to the same value")

# The fused result is a normal immutable array: further ops see the real value.
(assert (= (length (filter (fn [x] (> x 2)) [1 2 3 4 5])) 3)
        "fused result has the right length")
(assert (= (get (filter (fn [x] (> x 2)) [1 2 3 4 5]) 0) 3)
        "fused result indexes correctly")
(assert (= (mutable? (filter (fn [x] (> x 2)) [1 2 3])) false)
        "fused result is frozen")

# A capturing predicate is NOT fused, but must still compute correctly.
(assert (= (let [k 2]
             (filter (fn [x] (> x k)) [1 2 3 4])) [3 4])
        "capturing predicate (declined) is still correct")

# A MIXED map/filter chain fuses to ONE loop when every stage is reorder-safe
# (docs/impl/dissolution.md § "Mixed chains — one loop"; the full value/realization
# gauge is dissolution-mixed-fuse.lisp). Here the predicate is a variadic `>`, which
# routes through `apply` and is NOT reorder-safe, so the length-2 composition
# declines and the chain falls back to fusing only its inner reorder-safe run — the
# `filter` alone — leaving the outer `map` a plain call over the fused loop.
# (`lower_call`'s argument spill keeps that sound — call-arg-across-loop.lisp.) The
# VALUE is correct either way; the fallback is a realization difference, not a
# semantic one.
(assert (= (map (fn [x] (* x 10)) (filter (fn [x] (> x 2)) [1 2 3 4])) [30 40])
        "mixed map-of-filter (inner-only fallback) computes the same value")
(assert (= (filter (fn [x] (> x 20)) (map (fn [x] (* x 10)) [1 2 3])) [30])
        "mixed filter-of-map (inner-only fallback) computes the same value")

# ── Realization: the closure is gone ──────────────────────────────────
# A single `filter` over an inline non-capturing predicate mints no closure; the
# un-fused reference is a CAPTURING predicate (declined), which mints the closure.
# Both compute the same survivors. `arena/total-allocs` is a cumulative, monotonic
# count of objects ever minted (docs/impl/dissolution.md § "The gauge").
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def f-fused (allocs (fn [] (filter (fn [x] (> x 2)) [1 2 3 4 5 6 7 8 9 10]))))
(def f-unfused
  (allocs (fn []
            (let [m 2]
              (filter (fn [x] (> x m)) [1 2 3 4 5 6 7 8 9 10])))))
(assert (= (filter (fn [x] (> x 2)) [1 2 3 4 5 6 7 8 9 10])
           (let [m 2]
             (filter (fn [x] (> x m)) [1 2 3 4 5 6 7 8 9 10])))
        "fused and capturing-reference filters compute the same value")
(assert (< f-fused f-unfused)
        (string "fused filter must mint fewer (no closure): " f-fused " vs "
                f-unfused))

(println "dissolution-filter-fuse: ok")
