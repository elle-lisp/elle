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
# Each assertion compares FUSED (inline lambdas over a proven immutable array — the
# shape `fuse.rs` collapses) against an UN-FUSED reference computing the identical
# value through a named function with a `match` body. That body introduces a binding
# the inline-clone whitelist declines (docs/impl/dissolution.md § "Named same-unit
# functions"), so the call stays the real stdlib op: a per-element indirect call
# through a closure value and, in a composition, the staged intermediate array.
# Two shapes are NOT valid references here. A named fn with a whitelisted body
# inlines, and so does an inline lambda that CAPTURES a free variable — the splice
# is the call site, so its free variables are in scope (§ "Captures"). An inline
# lambda over a Var-bound array does not decline either (the gate follows the base
# through immutable aliases). Before fusion existed both sides were the same `map`
# calls and every delta was zero, so this file is its own counterfactual: it can
# only pass because fusion realizes the win.

# Cumulative objects minted while running `thunk`.
(defn allocs [thunk]
  (let [before (arena/total-allocs)]
    (thunk)
    (- (arena/total-allocs) before)))

(def base [0 1 2 3 4 5 6 7 8 9])

# The un-fused oracles: same value, `match` body, so each declines the inline clone
# and runs the real stdlib op.
(defn r-mul3 [x]
  (match x
    _ (* x 3)))
(defn r-inc [y]
  (match y
    _ (+ y 1)))
(defn r-dec1 [z]
  (match z
    _ (- z 1)))

# ── Composition: the intermediate collection is gone ──────────────────
# `(map g (map f xs))` fused is one loop; the un-fused reference runs both stdlib
# ops, so it mints the staged intermediate array on top of the per-element calls.
# Same value, strictly fewer allocations.
(def d2-fused
  (allocs (fn []
            (map (fn [y] (+ y 1)) (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9])))))
(def d2-unfused (allocs (fn [] (map r-inc (map r-mul3 [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 3)) base))
           (map r-inc (map r-mul3 base)))
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
  (allocs (fn [] (map r-dec1 (map r-inc (map r-mul3 [0 1 2 3 4 5 6 7 8 9]))))))
(assert (< d3-fused d3-unfused)
        "fused 3-deep tower mints fewer than the un-fused tower")
(assert (> (- d3-unfused d3-fused) (- d2-unfused d2-fused))
        "the saving scales with composition depth (one intermediate array per layer)")

# ── Single map: the per-element call is gone ──────────────────────────
# A single `map` over an inline lambda: fused splices `f`'s body into the loop, so
# no closure is called per element; the un-fused reference calls its declining
# oracle once per element. Both compute `x*3`. Same value, fewer allocations.
(def one-fused (allocs (fn [] (map (fn [x] (* x 3)) [0 1 2 3 4 5 6 7 8 9]))))
(def one-unfused (allocs (fn [] (map r-mul3 [0 1 2 3 4 5 6 7 8 9]))))
(assert (= (map (fn [x] (* x 3)) base) (map r-mul3 base))
        "fused and un-fused single map compute the same value")
(assert (< one-fused one-unfused)
        (string "fused single map must mint fewer: " one-fused " vs "
                one-unfused))

# The capture widening realizes the same win: an inline lambda reading an enclosing
# local fuses exactly as one reading only globals does — the splice is the call site
# (docs/impl/dissolution.md § "Captures"). Before that widening this was the
# un-fused reference above.
(def capture-fused
  (allocs (fn []
            (let [m 3]
              (map (fn [x] (* x m)) [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (let [m 3]
             (map (fn [x] (* x m)) base)) (map r-mul3 base))
        "the capturing map computes the stdlib value")
(assert (= capture-fused one-fused)
        (string "a capturing map fuses identically to a global-only one: "
                capture-fused " vs " one-fused))

# The Var-base widening realizes the win too: a `map` over an immutable array
# reached through a Var alias (`(let [v […]] (map f v))`) fuses exactly as the
# literal-base form does — the base need not be written at the call site. It mints
# the same as `one-fused` (no dispatch, no per-element call) and strictly fewer than
# the declining oracle.
(def var-fused
  (allocs (fn []
            (let [v [0 1 2 3 4 5 6 7 8 9]]
              (map (fn [x] (* x 3)) v)))))
(assert (= var-fused one-fused)
        (string "Var-base map fuses identically to literal-base: " var-fused
                " vs " one-fused))
(assert (< var-fused one-unfused)
        (string "Var-base map mints fewer than the un-fused oracle: " var-fused
                " vs " one-unfused))

# ── Cross-unit named fn: the intermediate collection is gone ──────────
# `(map dec (map dec xs))` where `dec` is a STDLIB `defn` (inlined across the
# compile-unit boundary, docs/impl/dissolution.md § "Cross-unit named functions")
# fuses to ONE loop — the intermediate array is gone. The un-fused reference is the
# same value behind declining oracles, so it runs both stdlib ops and mints the
# staged intermediate array. Same value, strictly fewer allocations — the
# realization win reaching across the compile-unit boundary.
(def xu-fused (allocs (fn [] (map dec (map dec [0 1 2 3 4 5 6 7 8 9])))))
(def xu-unfused (allocs (fn [] (map r-dec1 (map r-dec1 [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (map dec (map dec base)) (map r-dec1 (map r-dec1 base)))
        "fused cross-unit composition and the un-fused oracle agree")
(assert (< xu-fused xu-unfused)
        (string "fused cross-unit composition must mint strictly fewer objects: "
                xu-fused " vs " xu-unfused))

# ── Numeric intrinsic kernel: the opcode replaces the call ────────────
# A `(numeric!)`-declared raw-`%`-intrinsic body fuses (docs/impl/dissolution.md
# § "Raw `%`-intrinsic bodies"), so the numeric kernel — the shape a SIMD/GPU
# realization tier consumes — becomes one index-walk loop with the opcode inline
# and no per-element closure. The un-fused reference is the same kernel with the
# same declaration and the same opcode behind a `match` body, which declines the
# inline clone. Composed, the fused form additionally sheds the intermediate array,
# so its saving is strictly larger — the intermediate-elimination signature, not a
# fixed per-call constant.
(defn k-mul3 [x]
  (numeric!)
  (match x
    _ (%mul x 3)))
(defn k-inc [y]
  (numeric!)
  (match y
    _ (%add y 1)))

(def k1-fused
  (allocs (fn []
            (map (fn [x]
                   (numeric!)
                   (%mul x 3)) [0 1 2 3 4 5 6 7 8 9]))))
(def k1-unfused (allocs (fn [] (map k-mul3 [0 1 2 3 4 5 6 7 8 9]))))
(assert (= (map (fn [x]
                  (numeric!)
                  (%mul x 3)) base) (map k-mul3 base))
        "fused and un-fused numeric kernels compute the same value")
(assert (< k1-fused k1-unfused)
        (string "fused numeric kernel must mint strictly fewer objects: "
                k1-fused " vs " k1-unfused))

(def k2-fused
  (allocs (fn []
            (map (fn [y]
                   (numeric!)
                   (%add y 1))
                 (map (fn [x]
                        (numeric!)
                        (%mul x 3)) [0 1 2 3 4 5 6 7 8 9])))))
(def k2-unfused (allocs (fn [] (map k-inc (map k-mul3 [0 1 2 3 4 5 6 7 8 9])))))
(assert (= (map (fn [y]
                  (numeric!)
                  (%add y 1))
                (map (fn [x]
                       (numeric!)
                       (%mul x 3)) base)) (map k-inc (map k-mul3 base)))
        "fused and un-fused composed kernels compute the same value")
(assert (> (- k2-unfused k2-fused) (- k1-unfused k1-fused))
        (string "the composed kernel additionally sheds the intermediate array: "
                (- k2-unfused k2-fused) " vs " (- k1-unfused k1-fused)))

(println "dissolution-map-alloc: ok (d2 saved " (- d2-unfused d2-fused)
         ", d3 saved " (- d3-unfused d3-fused) ", cross-unit saved "
         (- xu-unfused xu-fused) ")")
