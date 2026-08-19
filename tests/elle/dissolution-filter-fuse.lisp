(elle/epoch 12)
# Filter loop fusion — value preservation + realization (docs/impl/dissolution.md).
#
# `(filter p xs)` over a proven immutable array with an inline
# predicate dissolves to an inlined index-walk loop with a GUARDED push: the
# element is bound once and pushed only when `(p item)` is truthy — no per-element
# closure, no `filter` dispatch. A `(filter q (filter p xs))` fuses to one loop
# with the guards nested (no intermediate array). This file is the behavioral
# gauge: the fused form must compute EXACTLY what the un-fused `filter` computes.
# The codegen gauge (that the closure and dispatch are gone and the push is
# guarded by an `if`) lives in `src/hir/typeinfer/fuse.rs`.
#
# The cross-check reference is `filter` applied to a named predicate with a
# `match`-body (`big?`): a top-level named fn with a PURE body now inlines too, and
# a `let` body inlines as well (docs § "Named same-unit functions"), so the un-fused
# oracle uses a `match` body — a binding-introducing form the inline-clone whitelist
# declines — to keep it a genuinely un-fused plain `filter` call. `big-let?` is the
# fusing let-body counterpart. Same survivors: fused inline-predicate, fused
# let-body, and the un-fused match-body form must agree.

(defn big? [x]
  (match x
    _ (> x 2)))

(defn big-let? [x]
  (let [y x]
    (> y 2)))

# Single filter: fused inline predicate == un-fused match-body pred == literal expect.
(assert (= (filter (fn [x] (> x 2)) [1 2 3 4]) [3 4])
        "single filter fuses to the same value")
(assert (= (filter (fn [x] (> x 2)) [1 2 3 4]) (filter big? [1 2 3 4]))
        "fused inline-predicate agrees with the un-fused match-body pred")
(assert (= (filter big-let? [1 2 3 4]) (filter big? [1 2 3 4]))
        "fused let-body predicate agrees with the un-fused match-body pred")

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

# The mutable-array arm (docs/impl/dissolution.md § "The mutable-array arm"): a
# single `filter` over a MUTABLE @array base fuses too, returning the surviving-
# element accumulator UNFROZEN — type-preserving, mirroring the stdlib arm. The
# survivor set is unchanged; only the result's mutability differs.
(let [m (filter (fn [x] (> x 2)) @[1 2 3 4])]
  (assert (= (mutable? m) true) "mutable-base filter returns an UNFROZEN array")
  (assert (= (length m) 2) "mutable-base filter keeps the survivors")
  (assert (= (get m 0) 3) "mutable-base filter first survivor")
  (assert (= (get m 1) 4) "mutable-base filter second survivor")
  (push m 5)
  (assert (= (get m 2) 5) "the unfrozen survivor array accepts an in-place push"))
(assert (= (mutable? (filter (fn [x] (> x 2)) [1 2 3 4])) false)
        "an immutable-base filter still returns a frozen array")

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

# A capturing predicate fuses too — the splice is the call site, so `k` is in scope
# (docs/impl/dissolution.md § "Captures").
(assert (= (let [k 2]
             (filter (fn [x] (> x k)) [1 2 3 4])) [3 4])
        "a capturing predicate fuses to the stdlib value")

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

# ── Realization: the per-element call is gone ─────────────────────────
# A single `filter` over an inline predicate splices the guard into the loop, so no
# closure is called per element; the un-fused reference calls its `match`-body
# oracle once per element. Both compute the same survivors. `arena/total-allocs` is
# a cumulative, monotonic count of objects ever minted (docs/impl/dissolution.md
# § "The gauge").
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def f-fused (allocs (fn [] (filter (fn [x] (> x 2)) [1 2 3 4 5 6 7 8 9 10]))))
(def f-unfused (allocs (fn [] (filter big? [1 2 3 4 5 6 7 8 9 10]))))
(assert (= (filter (fn [x] (> x 2)) [1 2 3 4 5 6 7 8 9 10])
           (filter big? [1 2 3 4 5 6 7 8 9 10]))
        "fused and un-fused filters compute the same value")
(assert (< f-fused f-unfused)
        (string "fused filter must mint fewer: " f-fused " vs " f-unfused))

# The capture widening realizes the same win: a predicate reading an enclosing local
# fuses exactly as one reading only globals does (docs/impl/dissolution.md
# § "Captures"). Before that widening this was the un-fused reference above.
(def f-capture
  (allocs (fn []
            (let [m 2]
              (filter (fn [x] (> x m)) [1 2 3 4 5 6 7 8 9 10])))))
(assert (= f-capture f-fused)
        (string "a capturing predicate fuses identically: " f-capture " vs "
                f-fused))

# A `(numeric!)`-declared raw-`%`-intrinsic predicate fuses too
# (docs/impl/dissolution.md § "Raw `%`-intrinsic bodies"): the declaration floors
# the parameter at Number, which discharges `%gt`'s comparable-family obligation,
# and the floor is carried onto the spliced binding — so the guard stage holds the
# opcode itself. The un-fused oracle carries the same declaration and opcode behind
# a `match` body, which declines the inline clone.
(defn big? [x]
  (numeric!)
  (%gt x 2))

(defn big-decl? [x]
  (numeric!)
  (match x
    _ (%gt x 2)))

(assert (= (filter big? [1 2 3 4]) [3 4])
        "a numeric!-declared intrinsic predicate fuses to the right survivors")
(assert (= (filter big? [1 2 3 4]) (filter big-decl? [1 2 3 4]))
        "the fused intrinsic predicate agrees with the un-fused oracle")

(println "dissolution-filter-fuse: ok")
