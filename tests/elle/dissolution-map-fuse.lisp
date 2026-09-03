(elle/epoch 12)
# Map-chain loop fusion — value preservation (docs/impl/dissolution.md).
#
# `(map f xs)` over a proven immutable array with an inline lambda
# `f` dissolves to an inlined index-walk loop, and `(map g (map f xs))` fuses to
# one loop with no intermediate array. This file is the behavioral gauge: the
# fused form must compute EXACTLY what the un-fused `map` computes. The codegen
# gauge (that the closure and dispatch are actually gone) lives in the Rust pins
# `src/hir/typeinfer/fuse.rs`; here we only assert the observable result is
# unchanged.
#
# `dbl`/`inc` are top-level named functions with pure bodies, so a `(map dbl xs)`
# also fuses (docs/impl/dissolution.md § "Named same-unit functions"): the
# function's body is grafted inline. A `let` body fuses too (a fragment closes
# over `let` bindings, so the graft re-mints them) — `dbl-let`/`inc-let` below are
# such fusing let-body oracles. The genuinely UN-fused cross-check oracle is
# `dbl-decl`/`inc-decl` — same value, but a `match` body cannot close (the
# admitted forms are the pure-expression ones plus `let`, not a `match` pattern),
# so it stays a plain `map` call. Fused inline-lambda, fused named-fn, fused
# let-body, and the un-fused match-body oracle must all agree.

(defn dbl [x]
  (* x 2))

(defn inc [x]
  (+ x 1))

# Fusing let-body named fns: a `let` body grafts inline, its own binding re-minted
# per call site. `dbl-let` uses a single binding; `inc-let` a SEQUENTIAL two-binding
# `let` whose second value references the first — the rename must rewrite that
# reference to the fresh id, so the value proves the sequential-rename order.
(defn dbl-let [x]
  (let [y (* x 2)]
    y))

(defn inc-let [x]
  (let [a (+ x 1)]
    (let [b (+ a 0)]
      b)))

# Un-fused oracles: a `match` body cannot close into a fragment, so these run
# the real stdlib `map`. Same value as `dbl`/`inc`.
(defn dbl-decl [x]
  (match x
    _ (* x 2)))

(defn inc-decl [x]
  (match x
    _ (+ x 1)))

# Single map: fused inline lambda == un-fused match-body oracle == literal.
(assert (= (map (fn [x] (* x 2)) [1 2 3]) [2 4 6])
        "single map fuses to the same value")
(assert (= (map (fn [x] (* x 2)) [1 2 3]) (map dbl-decl [1 2 3]))
        "fused inline-lambda agrees with the un-fused match-body oracle")

# Named-fn inlining: a `(map dbl xs)` fuses and agrees with the un-fused oracle.
(assert (= (map dbl [1 2 3]) [2 4 6]) "named-fn map fuses to the same value")
(assert (= (map dbl [1 2 3]) (map dbl-decl [1 2 3]))
        "fused named-fn agrees with the un-fused match-body oracle")
# A named fn is still usable as a first-class value after its inline at a call
# site (the graft copies it; the definition persists).
(assert (= (map dbl [1 2 3]) (map (fn [x] (dbl x)) [1 2 3]))
        "the inlined named fn is still callable as a value")

# Let-body named fns fuse (a fragment closes over `let` bindings), and must agree with
# the un-fused match-body oracle. `inc-let`'s SEQUENTIAL two-binding `let` (the
# second value reads the first) proves the rename rewrites the cross-binding
# reference to the fresh id.
(assert (= (map dbl-let [1 2 3]) (map dbl-decl [1 2 3]))
        "fused single-binding let-body agrees with the un-fused oracle")
(assert (= (map inc-let [1 2 3]) (map inc-decl [1 2 3]))
        "fused sequential-let-body agrees with the un-fused oracle")
(assert (= (map inc-let [1 2 3]) [2 3 4])
        "sequential-let-body fuses to the right value")

# Cross-unit named-fn inlining (docs/impl/dissolution.md § "Cross-unit named
# functions"): `dec` is a stdlib `defn` NOT redefined in this file, so `(map dec
# xs)` inlines a body carried across the compile-unit boundary. `dec-decl` is the
# un-fused oracle — a `match` body cannot close, so it runs the real stdlib
# `map`; fused and un-fused must agree.
(defn dec-decl [x]
  (match x
    _ (dec x)))
(assert (= (map dec [1 2 3]) [0 1 2])
        "cross-unit stdlib-fn map fuses to the right value")
(assert (= (map dec [1 2 3]) (map dec-decl [1 2 3]))
        "fused cross-unit stdlib fn agrees with the un-fused oracle")
(assert (= (map dec []) []) "cross-unit stdlib-fn map over empty fuses to empty")

# A cross-unit stdlib fn whose body is a `let`. Its `let` binding belongs to the
# stdlib's compile unit, so nothing here can read it: the fragment carries the
# binding itself (docs/impl/hir.md § "A fragment is closed over its bindings").
# The counter-factual is a compile that never runs — a splice reading the
# defining unit's arena indexes an arena this unit does not own.
(defn cfg-label-decl [c]
  (match c
    _ (fn/cfg-label c)))
(assert (= (map fn/cfg-label [{:name "a"} {:doc "d"} {}]) ["a" "d" "anonymous"])
        "a cross-unit let-body stdlib fn fuses to the right value")
(assert (= (map fn/cfg-label [{:name "a"} {:doc "d"} {}])
           (map cfg-label-decl [{:name "a"} {:doc "d"} {}]))
        "fused cross-unit let-body agrees with the un-fused oracle")

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
             (map (fn [x] (* x 2)) xs)) (map dbl-decl [1 2 3]))
        "fused Var-base agrees with the un-fused match-body oracle")

# Composition fuses to ONE loop; the interleaved order is unobservable for these
# pure transforms, and the value matches the staged `map`-of-`map`. The un-fused
# oracle uses the match-body fns so it runs the real staged stdlib maps.
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3])) [3 5 7])
        "map-of-map fuses to the same value")
(assert (= (map (fn [y] (+ y 1)) (map (fn [x] (* x 2)) [1 2 3]))
           (map inc-decl (map dbl-decl [1 2 3])))
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

# A capturing lambda fuses — the splice is the call site, so `k` is in scope
# (docs/impl/dissolution.md § "Captures").
(assert (= (let [k 100]
             (map (fn [x] (+ x k)) [1 2 3])) [101 102 103])
        "a capturing lambda fuses to the stdlib value")

# A raw call-position `%`-intrinsic body FUSES under a `(numeric!)` declaration
# (docs/impl/dissolution.md § "Raw `%`-intrinsic bodies"). The declaration floors
# the parameter at Number — the sole proof that discharges `(%add x 1)` — and it
# is recorded on the parameter BINDING, so it survives the splice that dissolves
# the lambda. Two things are asserted at once: the file compiles at all (an
# uncarried floor would make the spliced `%add` unprovable — a compile error), and
# the fused value equals the un-fused oracle's. `sq-decl` is that oracle: same
# declaration and same opcode, but a `match` body cannot close into a fragment, so it
# runs the real stdlib `map`.
(defn sq [x]
  (numeric!)
  (%mul x x))

(defn sq-decl [x]
  (numeric!)
  (match x
    _ (%mul x x)))

(assert (= (map (fn [x]
                  (numeric!)
                  (%add x 1)) [1 2 3]) [2 3 4])
        "a numeric!-declared intrinsic kernel fuses to the right value")
(assert (= (map sq [1 2 3]) [1 4 9])
        "a named numeric kernel inlines to the right value")
(assert (= (map sq [1 2 3]) (map sq-decl [1 2 3]))
        "the fused numeric kernel agrees with the un-fused match-body oracle")
(assert (= (map sq []) []) "numeric kernel over an empty array fuses to empty")

# The div family carries a second obligation — a provably nonzero divisor — which
# here is the literal `2`, part of the body and untouched by the splice.
(assert (= (map (fn [x]
                  (numeric!)
                  (%div x 2)) [4 6 8]) [2 3 4])
        "a %div kernel with a literal divisor fuses to the right value")

# Both fold parameters carry the floor, so the spliced step proves over the
# accumulator and the element alike.
(assert (= (fold (fn [a x]
                   (numeric!)
                   (%add a x)) 0 [1 2 3]) 6)
        "a numeric!-declared intrinsic combinator fuses to the right value")

# A composed pair of kernels fuses to one loop (an intrinsic body is silent, so it
# is reorder-safe); the value matches the staged un-fused oracle.
(assert (= (map (fn [y]
                  (numeric!)
                  (%add y 1))
                (map (fn [x]
                       (numeric!)
                       (%mul x 2)) [1 2 3])) [3 5 7])
        "composed numeric kernels fuse to the same value")

# Without the declaration there is no floor to carry, so an intrinsic body
# DECLINES even when its operands are literals — and must still compute correctly.
(assert (= (map (fn [x] (%add 1 2)) [1 2 3]) [3 3 3])
        "an undeclared intrinsic body (declined) is still correct")

(println "dissolution-map-fuse: ok")
