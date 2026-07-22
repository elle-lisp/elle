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
# Each assertion compares FUSED (inline non-capturing lambdas over a literal
# array — the shape `fuse.rs` collapses) against an UN-FUSED reference computing
# the identical value (named top-level fns, or an inline lambda over a Var base —
# shapes the gate declines). Before fusion existed both sides were the same `map`
# calls and every delta was zero, so this file is its own counterfactual: it can
# only pass because fusion realizes the win.

(defn f3 [x]
  (* x 3))
(defn g1 [y]
  (+ y 1))
(defn hm1 [z]
  (- z 1))

# Cumulative objects minted while running `thunk`.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def base [0 1 2 3 4 5 6 7 8 9])

# ── Composition: the intermediate collection is gone ──────────────────
# `(map g (map f xs))` fused is one loop; the un-fused reference (named fns) mints
# an intermediate array too. Same value, strictly fewer allocations.
(def d2-fused
  (allocs (fn []
            (map (fn [y] (+ y 1)) (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9])))))
(def d2-unfused (allocs (fn [] (map g1 (map f3 [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 3)) base))
           (map g1 (map f3 base)))
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
  (allocs (fn [] (map hm1 (map g1 (map f3 [0 1 2 3 4 5 6 7 8 9]))))))
(assert (< d3-fused d3-unfused)
        "fused 3-deep tower mints fewer than the un-fused tower")
(assert (> (- d3-unfused d3-fused) (- d2-unfused d2-fused))
        "the saving scales with composition depth (one intermediate array per layer)")

# ── Single map: the closure is gone ───────────────────────────────────
# A single `map` over an inline lambda: fused inlines `f` (no closure minted); the
# un-fused reference is the same inline lambda over a Var base (a shape the gate
# declines), which mints the closure. Same value, fewer allocations.
(def one-fused (allocs (fn [] (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9]))))
(def one-unfused
  (allocs (fn []
            (let [v [0 1 2 3 4 5 6 7 8 9]]
              (map (fn [x] (* x 3)) v)))))
(assert (< one-fused one-unfused)
        (string "fused single map must mint fewer (no closure): " one-fused
                " vs " one-unfused))

(println "dissolution-map-alloc: ok (d2 saved " (- d2-unfused d2-fused)
         ", d3 saved " (- d3-unfused d3-fused) ")")
