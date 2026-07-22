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
# The cross-check reference is `map` applied to a NAMED function (`dbl`) — a
# Var, not a lambda literal, so the gate leaves it a plain `map` call. Fused
# inline-lambda and un-fused named-fn must agree.

(defn dbl [x]
  (* x 2))

(defn inc [x]
  (+ x 1))

# Single map: fused inline lambda == un-fused named fn == literal expectation.
(assert (= (map (fn [x] (* x 2)) [1 2 3]) [2 4 6])
        "single map fuses to the same value")
(assert (= (map (fn [x] (* x 2)) [1 2 3]) (map dbl [1 2 3]))
        "fused inline-lambda agrees with un-fused named-fn")

# Boundary sizes.
(assert (= (map (fn [x] (* x 2)) []) []) "empty array fuses to empty")
(assert (= (map (fn [x] (* x 2)) [7]) [14]) "singleton array fuses")

# A parameter used more than once in the body — the element must be evaluated
# once and bound, not re-substituted.
(assert (= (map (fn [x] (+ x x)) [10 20 30]) [20 40 60])
        "multi-use parameter fuses correctly")

# Composition fuses to ONE loop; the interleaved order is unobservable for these
# pure transforms, and the value matches the staged `map`-of-`map`.
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3])) [3 5 7])
        "map-of-map fuses to the same value")
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3]))
           (map inc (map dbl [1 2 3])))
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
