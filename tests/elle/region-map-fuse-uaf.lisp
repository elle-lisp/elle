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

# Loop the whole thing so repeated fused mints/frees exercise region-id churn:
# a stale accumulator or base region would be reused and fault on a later pass.
(def @acc "")
(def @i 0)
(while (< i 50)
  (let [r (map (fn [x] (string "r" x)) [1 2 3])]
    (assign acc (get r 0)))
  (assign i (+ i 1)))
(assert (= acc "r1") "repeated fused loops stay sound under region-id churn")

(println "region-map-fuse-uaf: ok")
