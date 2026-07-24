(elle/epoch 12)
# The REALIZATION gauge for map-chain fusion (docs/impl/dissolution.md § "The gauge").
#
# The codegen pins (`src/hir/typeinfer/fuse.rs`) prove the fused form's STRUCTURE
# (the `map` dispatch and the closure are gone, the composed case has one
# accumulator). This file proves the EXECUTION consequence the mission actually
# cares about (`memory.md §1`: "fewer allocations… which the leak oracle does not
# observe"): a fused chain MINTS STRICTLY FEWER heap objects per call.
#
# The instrument is `arena/total-allocs` — a CUMULATIVE, monotonic count of
# objects ever minted. The intermediate array a `map`-of-`map` builds is
# non-escaping and freed before the call returns, so it is invisible to every
# live/peak/steady-state axis (the leak oracle included); only a cumulative
# allocation-event count sees it. The count is deterministic (independent of GC
# timing), so these are exact `<` relations, not statistical bounds.
#
# Each assertion compares FUSED (inline non-capturing lambdas over a proven
# immutable array — the shape `fuse.rs` collapses) against an UN-FUSED reference
# computing the identical value via an inline lambda that CAPTURES a free variable
# — a shape the gate declines (splicing a capture at the call site is out of
# scope), so it still mints the per-call closure and, in a composition, the staged
# intermediate array. A named top-level fn is NOT a valid un-fused reference here:
# those now inline too (docs/impl/dissolution.md § "Named same-unit functions"). An
# inline lambda over a Var-bound array also does NOT decline (the gate follows the
# base through immutable aliases). Before fusion existed both sides were the same
# `map` calls and every delta was zero, so this file is its own counterfactual: it
# can only pass because fusion realizes the win.

# Cumulative objects minted while running `thunk`.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def base [0 1 2 3 4 5 6 7 8 9])

# ── Composition: the intermediate collection is gone ──────────────────
# `(map g (map f xs))` fused is one loop; the un-fused reference (capturing
# lambdas) mints per-call closures and an intermediate array too. Same value,
# strictly fewer allocations.
(def d2-fused
  (allocs (fn []
            (map (fn [y] (+ y 1)) (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9])))))
(def d2-unfused
  (allocs (fn []
            (let [m 3
                  n 1]
              (map (fn [y] (+ y n)) (map (fn [x] (* x m)) [0 1 2 3 4 5 6 7 8 9]))))))
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 3)) base))
           (let [m 3
                 n 1]
             (map (fn [y] (+ y n)) (map (fn [x] (* x m)) base))))
        "fused and un-fused composition compute the same value")
(assert (< d2-fused d2-unfused)
        (string "fused composition must mint strictly fewer objects: " d2-fused
                " vs " d2-unfused))

# The saving is one intermediate array per fused layer: a 3-deep tower saves
# STRICTLY MORE than a 2-deep one (the intermediate-elimination signature — not a
# one-off constant that a single removed alloc would also satisfy).
(def d3-fused
  (allocs (fn []
            (map (fn [z] (- z 1))
                 (map (fn [y] (+ y 1))
                      (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9]))))))
(def d3-unfused
  (allocs (fn []
            (let [m 3
                  n 1
                  p 1]
              (map (fn [z] (- z p))
                   (map (fn [y] (+ y n))
                        (map (fn [x] (* x m)) [0 1 2 3 4 5 6 7 8 9])))))))
(assert (< d3-fused d3-unfused)
        "fused 3-deep tower mints fewer than the un-fused tower")
(assert (> (- d3-unfused d3-fused) (- d2-unfused d2-fused))
        "the saving scales with composition depth (one intermediate array per layer)")

# ── Single map: the closure is gone ───────────────────────────────────
# A single `map` over an inline non-capturing lambda: fused inlines `f` (no closure
# minted); the un-fused reference is a CAPTURING lambda (a shape the gate declines,
# because splicing a capture at the call site is out of scope), which mints the
# closure. Both compute `x*3`. Same value, fewer allocations.
(def one-fused (allocs (fn [] (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9]))))
(def one-unfused
  (allocs (fn []
            (let [m 3]
              (map (fn [x] (* x m)) [0 1 2 3 4 5 6 7 8 9])))))
(assert (< one-fused one-unfused)
        (string "fused single map must mint fewer (no closure): " one-fused
                " vs " one-unfused))

# The Var-base widening realizes the win too: a `map` over an immutable array
# reached through a Var alias (`(let [v […]] (map f v))`) fuses exactly as the
# literal-base form does — the base need not be written at the call site. It mints
# the same as `one-fused` (no closure, no dispatch) and strictly fewer than the
# capturing reference. This is the realization gauge for the new pattern: before
# the widening this Var-base form was the un-fused reference above.
(def var-fused
  (allocs (fn []
            (let [v [0 1 2 3 4 5 6 7 8 9]]
              (map (fn [x] (* x 3)) v)))))
(assert (= var-fused one-fused)
        (string "Var-base map fuses identically to literal-base: " var-fused
                " vs " one-fused))
(assert (< var-fused one-unfused)
        (string "Var-base map mints fewer than the capturing reference: "
                var-fused " vs " one-unfused))

# ── Cross-unit named fn: the intermediate collection is gone ──────────
# `(map dec (map dec xs))` where `dec` is a STDLIB `defn` (inlined across the
# compile-unit boundary, docs/impl/dissolution.md § "Cross-unit named functions")
# fuses to ONE loop — the intermediate array is gone. The un-fused reference uses
# CAPTURING lambdas computing the same value (a shape the gate declines), so it
# mints per-call closures AND the staged intermediate array. Same value, strictly
# fewer allocations — the realization win reaching across the compile-unit boundary.
(def xu-fused (allocs (fn [] (map dec (map dec [0 1 2 3 4 5 6 7 8 9])))))
(def xu-unfused
  (allocs (fn []
            (let [k 1]
              (map (fn [y] (- y k)) (map (fn [x] (- x k)) [0 1 2 3 4 5 6 7 8 9]))))))
(assert (= (map dec (map dec base))
           (let [k 1]
             (map (fn [y] (- y k)) (map (fn [x] (- x k)) base))))
        "fused cross-unit composition and the capturing reference agree")
(assert (< xu-fused xu-unfused)
        (string "fused cross-unit composition must mint strictly fewer objects: "
                xu-fused " vs " xu-unfused))

(println "dissolution-map-alloc: ok (d2 saved " (- d2-unfused d2-fused)
         ", d3 saved " (- d3-unfused d3-fused) ", cross-unit saved "
         (- xu-unfused xu-fused) ")")
