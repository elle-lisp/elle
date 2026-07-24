(elle/epoch 12)
# Map-chain loop fusion — value preservation (docs/impl/dissolution.md).
#
# `(map f xs)` over a proven immutable array with an inline non-capturing lambda
# `f` dissolves to an inlined index-walk loop, and `(map g (map f xs))` fuses to
# one loop with no intermediate array. This file is the behavioral gauge: the
# fused form must compute EXACTLY what the un-fused `map` computes. The codegen
# gauge (that the closure and dispatch are actually gone) lives in the Rust pins
# `src/hir/typeinfer/fuse.rs`; here we only assert the observable result is
# unchanged.
#
# `dbl`/`inc` are top-level named functions with pure bodies, so a `(map dbl xs)`
# now ALSO fuses (docs/impl/dissolution.md § "Named same-unit functions"): the
# function's body is cloned inline. The genuinely UN-fused cross-check oracle is
# `dbl-let`/`inc-let` — same value, but a `let`-body declines the inline clone
# (the whitelist covers only pure-expression forms), so it stays a plain `map`
# call. Fused inline-lambda, fused named-fn, and the un-fused let-body oracle must
# all agree.

(defn dbl [x]
  (* x 2))

(defn inc [x]
  (+ x 1))

# Un-fused oracles: a `let`-body declines the named-fn inline clone, so these run
# the real stdlib `map`. Same value as `dbl`/`inc`.
(defn dbl-let [x]
  (let [y x]
    (* y 2)))

(defn inc-let [x]
  (let [y x]
    (+ y 1)))

# Single map: fused inline lambda == un-fused let-body oracle == literal.
(assert (= (map (fn [x] (* x 2)) [1 2 3]) [2 4 6])
        "single map fuses to the same value")
(assert (= (map (fn [x] (* x 2)) [1 2 3]) (map dbl-let [1 2 3]))
        "fused inline-lambda agrees with the un-fused let-body oracle")

# Named-fn inlining: a `(map dbl xs)` fuses and agrees with the un-fused oracle.
(assert (= (map dbl [1 2 3]) [2 4 6]) "named-fn map fuses to the same value")
(assert (= (map dbl [1 2 3]) (map dbl-let [1 2 3]))
        "fused named-fn agrees with the un-fused let-body oracle")
# A named fn is still usable as a first-class value after its inline at a call
# site (the inline clones it; the definition persists).
(assert (= (map dbl [1 2 3]) (map (fn [x] (dbl x)) [1 2 3]))
        "the inlined named fn is still callable as a value")

# Boundary sizes.
(assert (= (map (fn [x] (* x 2)) []) []) "empty array fuses to empty")
(assert (= (map (fn [x] (* x 2)) [7]) [14]) "singleton array fuses")

# A parameter used more than once in the body — the element must be evaluated
# once and bound, not re-substituted.
(assert (= (map (fn [x] (+ x x)) [10 20 30]) [20 40 60])
        "multi-use parameter fuses correctly")

# The base collection reached through a Var alias fuses just as a call-site
# literal does (the gate follows the base through immutable, unmutated aliases to
# the proven array). Value must be unchanged.
(assert (= (let [xs [1 2 3]]
             (map (fn [x] (* x 2)) xs)) [2 4 6])
        "map over a Var-bound immutable array fuses to the same value")
(assert (= (let [xs [1 2 3]]
             (let [ys xs]
               (map (fn [x] (* x 2)) ys))) [2 4 6])
        "map over an aliased Var fuses to the same value")
(assert (= (let [xs [1 2 3]]
             (map (fn [x] (* x 2)) xs)) (map dbl-let [1 2 3]))
        "fused Var-base agrees with the un-fused let-body oracle")

# Composition fuses to ONE loop; the interleaved order is unobservable for these
# pure transforms, and the value matches the staged `map`-of-`map`. The un-fused
# oracle uses the let-body fns so it runs the real staged stdlib maps.
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3])) [3 5 7])
        "map-of-map fuses to the same value")
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3]))
           (map inc-let (map dbl-let [1 2 3])))
        "fused composition agrees with the un-fused staged maps")

# A three-deep tower — the intermediate arrays all dissolve.
(assert (= (map (fn [z] (- z 1))
                (map (fn [y] (+ y 1)) (map (fn [x] (* x 10)) [1 2 3])))
           [10 20 30]) "three-deep tower fuses to the same value")

# The fused result is a normal immutable array: further ops see the real value.
(assert (= (length (map (fn [x] (* x 2)) [1 2 3 4 5])) 5)
        "fused result has the right length")
(assert (= (get (map (fn [x] (* x 2)) [5 6 7]) 1) 12)
        "fused result indexes correctly")

# The result is immutable (map is type-preserving over an immutable array).
(assert (= (mutable? (map (fn [x] (* x 2)) [1 2 3])) false)
        "fused result is frozen")

# The mutable-array arm (docs/impl/dissolution.md § "The mutable-array arm"): a
# single `map` over a MUTABLE @array base fuses too, but returns the accumulator
# UNFROZEN — type-preserving, mirroring the stdlib arm (if (mutable? coll) acc
# (freeze acc)). The element values are unchanged; only the result's mutability
# differs from the immutable-base arm.
(let [m (map (fn [x] (* x 2)) @[1 2 3])]
  (assert (= (mutable? m) true) "mutable-base map returns an UNFROZEN array")
  (assert (= (length m) 3) "mutable-base map result has the right length")
  (assert (= (get m 0) 2) "mutable-base map first element")
  (assert (= (get m 2) 6) "mutable-base map last element")
  # Genuinely mutable — a further push mutates the result in place.
  (push m 99)
  (assert (= (length m) 4) "the unfrozen result accepts an in-place push")
  (assert (= (get m 3) 99) "the pushed element is present"))

# The mutable arm reaches a Var-bound @array too (the same alias proof, resolving
# to the @array keyword instead of array).
(let [xs @[5 6 7]]
  (let [m (map (fn [x] (+ x 1)) xs)]
    (assert (= (mutable? m) true) "Var-bound mutable-base map is unfrozen")
    (assert (= (get m 1) 7) "Var-bound mutable-base map value")))

# A capturing lambda is NOT fused, but must still compute correctly.
(assert (= (let [k 100]
             (map (fn [x] (+ x k)) [1 2 3])) [101 102 103])
        "capturing lambda (declined) is still correct")

# A raw call-position `%`-intrinsic body is DECLINED, not broken: `(numeric!)`
# floors the LAMBDA PARAMETER at Number, the sole proof that discharges
# `(%add x 1)` — a floor that cannot survive inlining (no lambda, no param). So
# the pass must leave this a plain `map` call; it must still compile and compute
# the same value. (Fusing it is headroom needing element-type inference through
# `get`; the codegen decline is pinned in `src/hir/typeinfer/fuse.rs`.)
(assert (= (map (fn [x]
                  (numeric!)
                  (%add x 1)) [1 2 3]) [2 3 4])
        "raw-intrinsic body (declined) still compiles and computes correctly")

(println "dissolution-map-fuse: ok")
