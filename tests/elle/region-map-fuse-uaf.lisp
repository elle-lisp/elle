(elle/epoch 12)
# Guardfree soundness of map-chain loop fusion (docs/impl/dissolution.md).
#
# `(map f xs)` / `(map g (map f xs))` over a proven immutable array with inline
# non-capturing lambdas dissolves to one inlined index-walk loop. The loop mints
# a fresh @array accumulator, fills it with the per-element results, and freezes
# it; the base array (a fresh literal here) is owned by the loop's `coll` binding
# and freed at scope exit. This fixture drives that path with HEAP element values
# — strings and structs — so that any over-free (a base element freed under the
# loop's read, or an accumulator member freed before the result is consumed)
# faults at the exact access under `--trace=guardfree`, rather than leaking
# silently. The plain-VM run also asserts the values, so a miscompile is loud
# either way.

# Single map producing heap strings; the results are read back after the loop.
(def ss (map (fn [x] (string "n" x)) [1 2 3]))
(assert (= ss ["n1" "n2" "n3"]) "single map over heap results")
(assert (= (get ss 2) "n3") "fused heap element survives to a later read")

# Single map producing heap structs, then read a field back out.
(def structs (map (fn [x] {:v x :s (string "k" x)}) [10 20 30]))
(assert (= (get (get structs 1) :v) 20) "fused struct element field read")
(assert (= (get (get structs 1) :s) "k20") "fused struct heap field read")

# Composition producing a heap chain — the intermediate string array dissolves,
# and the outer result's heap members must outlive the (gone) intermediate.
(def chained (map (fn [s] (string s "!")) (map (fn [x] (string "v" x)) [1 2 3])))
(assert (= chained ["v1!" "v2!" "v3!"]) "fused composition over heap values")
(assert (= (get chained 0) "v1!") "fused composition heap element survives")

# The base array elements are themselves heap values (strings), read through the
# transform — the loop must not free a base element under its own (get coll i).
(def upper (map (fn [w] (string/upcase w)) ["ab" "cd" "ef"]))
(assert (= upper ["AB" "CD" "EF"]) "fused map over a heap-element base array")

# The Var-base widening under heap values: the base array is bound to a Var (a
# heap-element literal), then mapped. The fused loop reads `coll` — the aliased
# base — by index; an over-free of the base under the loop's own read, or of the
# base binding while the accumulator still holds derived heap members, faults here
# under --trace=guardfree. The base Var is also read AFTER the map, so it must
# survive the loop, not be consumed by it.
(def base ["p" "q" "r"])
(def viamap (map (fn [s] (string s s)) base))
(assert (= viamap ["pp" "qq" "rr"]) "Var-base fused map over heap elements")
(assert (= (get base 1) "q")
        "the base Var survives the fused loop (not consumed)")

# The mutable-array arm: over a @array base the fused loop returns the
# accumulator UNFROZEN (docs/impl/dissolution.md § "The mutable-array arm").
# Driving it with heap element values, then reading them back and mutating the
# result in place: an over-free of a base element under the loop's own read, or of
# an accumulator member before the result is consumed, faults here under
# --trace=guardfree. The unfrozen result is genuinely mutable, so a later push
# exercises the live @array on the region path.
(def mm (map (fn [x] (string "m" x)) @[1 2 3]))
(assert (= (mutable? mm) true)
        "mutable-base fused map returns an unfrozen array")
(assert (= (get mm 0) "m1") "mutable-base fused map over heap elements")
(assert (= (get mm 2) "m3") "mutable-base fused heap element survives to a read")
(push mm (string "m" 4))
(assert (= (get mm 3) "m4") "the unfrozen heap result accepts an in-place push")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn:
# a stale accumulator or base region would be reused and fault on a later pass.
(def @acc "")
(def @i 0)
(while (< i 50)
  (let [r (map (fn [x] (string "r" x)) [1 2 3])]
    (assign acc (get r 0)))
  (assign i (+ i 1)))
(assert (= acc "r1") "repeated fused loops stay sound under region-id churn")

# Named same-unit function inlining (docs/impl/dissolution.md § "Named same-unit
# functions"): a `(map wrap xs)` naming a top-level `defn` inlines `wrap`'s body by
# CLONING it with fresh bindings and HirIds. Drive the cloned body with heap
# element values (strings) read back after the loop — an over-free of a base
# element under the cloned body's read, or of an accumulator member before the
# result is consumed, faults here under --trace=guardfree. The definition persists
# (it is cloned, not moved), so it is also called directly afterward.
(defn wrap [s]
  (string "<" s ">"))
(def wrapped (map wrap ["a" "b" "c"]))
(assert (= wrapped ["<a>" "<b>" "<c>"]) "named-fn inline over heap elements")
(assert (= (get wrapped 2) "<c>")
        "named-fn inlined heap element survives a read")
(assert (= (wrap "z") "<z>") "the inlined definition is still callable directly")

# A named 2-arg combinator inlined into a fold over heap accumulators: the fold
# rebuilds a heap string each step, so the displaced prior must survive the read
# that builds its successor.
(defn joinf [acc s]
  (string acc s))
(def joined (fold joinf "" ["p" "q" "r"]))
(assert (= joined "pqr") "named-combinator fold over heap accumulators")

(println "region-map-fuse-uaf: ok")
