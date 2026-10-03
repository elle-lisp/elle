(elle/epoch 12)
# audited: 2026-09-29
# A fused `filter`, lone or composed, computes exactly the survivors the un-fused
# `filter` computes, and mints fewer objects.
#
# docs/impl/dissolution/stages.md
#
# The codegen pins — the closure and dispatch gone, the push guarded by an `if` —
# are in src/hir/typeinfer/fuse/tests/collect.rs.
#
# The un-fused oracle `big?` has a `match` body — a binding-introducing form a
# fragment cannot close over — so it stays a plain `filter` call. A pure body or a
# `let` body inlines, so `big-let?` is the fusing counterpart. The fused
# inline predicate, the fused let-body one and the un-fused oracle must agree.

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

# A single `filter` over a MUTABLE @array base fuses too, and returns the
# survivor accumulator UNFROZEN, as the stdlib arm does. The survivor set is
# unchanged; only the result's mutability differs.
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

# A capturing predicate fuses too — the splice is the call site, so `k` is in
# scope there.
(assert (= (let [k 2]
             (filter (fn [x] (> x k)) [1 2 3 4])) [3 4])
        "a capturing predicate fuses to the stdlib value")

# A mixed map/filter chain with a variadic `>` predicate: `>` routes through
# `apply` and is not reorder-safe, so the length-2 composition declines and only
# the inner `filter` fuses, leaving the outer `map` a plain call over the fused
# loop. `lower_call`'s argument spill keeps that sound (call-arg-across-loop.lisp).
# The value is the same either way. dissolution-mixed-fuse.lisp weighs the
# reorder-safe mixed chains.
(assert (= (map (fn [x] (* x 10)) (filter (fn [x] (> x 2)) [1 2 3 4])) [30 40])
        "mixed map-of-filter (inner-only fallback) computes the same value")
(assert (= (filter (fn [x] (> x 20)) (map (fn [x] (* x 10)) [1 2 3])) [30])
        "mixed filter-of-map (inner-only fallback) computes the same value")

# ── Realization: the per-element call is gone ─────────────────────────
# A single `filter` over an inline predicate splices the guard into the loop, so no
# closure is called per element; the un-fused reference calls its `match`-body
# oracle once per element. Both compute the same survivors. `arena/total-allocs`
# counts every object ever minted.
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

# A predicate reading an enclosing local fuses exactly as one reading only globals
# does, so it mints the same count.
(def f-capture
  (allocs (fn []
            (let [m 2]
              (filter (fn [x] (> x m)) [1 2 3 4 5 6 7 8 9 10])))))
(assert (= f-capture f-fused)
        (string "a capturing predicate fuses identically: " f-capture " vs "
                f-fused))

# A `(numeric!)`-declared raw-`%`-intrinsic predicate fuses too
# (docs/impl/dissolution/inline.md): the declaration floors the parameter at
# Number, which discharges `%gt`'s comparable-family obligation, and the floor is
# carried onto the spliced binding — so the guard stage holds the opcode itself.
# The un-fused oracle carries the same declaration and opcode behind a `match`
# body, which cannot close into a fragment.
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
