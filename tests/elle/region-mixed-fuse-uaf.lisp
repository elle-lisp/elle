(elle/epoch 12)
# Guardfree soundness of MIXED map/filter loop fusion (docs/impl/dissolution.md
# § "Mixed chains — one loop").
#
# A mixed `(map f (filter p xs))` / `(filter q (map g xs))` over a proven immutable
# array with inline non-capturing lambdas fuses to ONE index-walk loop: a `map`
# stage transforms the threaded element, a `filter` stage binds it once and pushes
# under a guard, and the intermediate array between the two ops never exists. This
# fixture drives that path with HEAP element values — strings and structs — so that
# any over-free (a base element freed under the loop's read in a transform or a
# guard, or an accumulator member freed before the frozen result is consumed) faults
# at the exact access under --trace=guardfree, rather than leaking silently. The
# plain-VM run also asserts the values, so a miscompile is loud either way. The
# lambdas are reorder-safe (`string?`/`integer?` type tests, `string`/`length`
# transforms), so the composition fuses into one loop rather than the inner-only
# fallback.

# map-of-filter over heap strings: keep the strings (guard reads each heap element),
# then transform the survivors into fresh heap strings pushed into the accumulator.
# A dropped element (the integer) must not free a kept heap string, and vice-versa.
(def kept
  (map (fn [s] (string s "!"))
       (filter (fn [s] (string? s)) ["a" 1 "bb" 2 "ccc"])))
(assert (= kept ["a!" "bb!" "ccc!"]) "mixed map-of-filter over heap strings")
(assert (= (get kept 2) "ccc!")
        "fused mixed heap survivor survives a later read")

# filter-of-map: transform each element into a fresh heap string FIRST, then guard
# the transformed value. The guard reads the mapped heap value; the accumulator
# holds the mapped heap strings, which must outlive the (gone) intermediate array.
(def mapped (filter (fn [s] (string? s)) (map (fn [x] (string "n" x)) [1 2 3])))
(assert (= mapped ["n1" "n2" "n3"]) "mixed filter-of-map producing heap values")
(assert (= (get mapped 0) "n1") "fused mixed mapped heap element survives")

# Mixed over heap structs: filter by a heap field's type, then read a field of the
# survivor back out through the map. The guard and the transform both dereference
# the heap struct; a dropped struct must not free a kept one.
(def structs
  (map (fn [r] (get r :s))
       (filter (fn [r] (string? (get r :s)))
               [{:v 1 :s "x"} {:v 2 :s "y"} {:v 3 :s "z"}])))
(assert (= structs ["x" "y" "z"]) "mixed over heap structs, field read back out")
(assert (= (get structs 1) "y") "fused mixed struct heap field survives")

# The base array is bound to a Var (heap-element literal, mixed types) and read
# AFTER the mixed loop, so the fused loop must not consume/free the base — it reads
# `coll` by index and the base outlives the loop. The `string?` guard drops the
# integers; the survivors are transformed into fresh heap strings.
(def base ["p" 0 "qq" 0 "rrr"])
(def viamix (map (fn [s] (string s s)) (filter (fn [s] (string? s)) base)))
(assert (= viamix ["pp" "qqqq" "rrrrrr"])
        "Var-base mixed fused loop over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused loop (not consumed)")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale accumulator or base region would be reused and fault on a later pass.
(def @acc "")
(def @i 0)
(while (< i 50)
  (let [r (map (fn [s] (string s "z"))
               (filter (fn [s] (string? s)) ["keep" 7 "q"]))]
    (assign acc (get r 0)))
  (assign i (+ i 1)))
(assert (= acc "keepz")
        "repeated fused mixed loops stay sound under region-id churn")

(println "region-mixed-fuse-uaf: ok")
