(elle/epoch 12)
# Guardfree soundness of COUNT loop fusion (docs/impl/dissolution.md
# § "Count — the terminal that is a guard plus a tally").
#
# `(count pred xs)` dissolves to an index-walk loop whose last stage is the
# predicate's guard and whose base case tallies a scalar. The element value the
# tally receives is discarded, so the base's heap elements must stay live for the
# guard that reads them — and, over a map prefix, the freshly-minted heap value
# each element is transformed into must live from its creation to the guard's read
# in the SAME iteration, with no intermediate array to hold it. This fixture drives
# that path with heap values in every role, so an over-free faults at the exact
# access under --trace=guardfree rather than leaking silently. The plain-VM run
# also asserts the values, so a miscompile is loud either way.
#
# The composition cases use single-primitive predicates (`string?`, `empty?`,
# `number?`): a variadic comparison like `>` routes through `apply` and is not
# reorder-safe, so it would decline the composition and gauge nothing. `>` appears
# only where it is meant to — the LONE counts, which carry no reorder gate, and the
# fallback case below.

# Heap elements read by the predicate: a base string must not be freed under the
# guard that measures it, nor under the following iteration's read. A lone count
# has no reorder gate, so the variadic `>` still fuses here.
(def long-ones (count (fn [s] (> (length s) 1)) ["a" "bb" "ccc" "d"]))
(assert (= long-ones 2) "count over heap-string elements")

# Heap struct elements: the predicate dereferences a heap field per element.
(def rich (count (fn [r] (> (get r :v) 15)) [{:v 10} {:v 20} {:v 30}]))
(assert (= rich 2) "count reading a heap struct field per element")

# count-of-map over heap: the map stage builds a FRESH heap string per element
# that the guard immediately reads. With no intermediate array the mapped value is
# reachable only through the loop's own local — it must survive the guard's read.
(def prefixed
  (count (fn [s] (string? s)) (map (fn [x] (string "v" x)) [1 22 333])))
(assert (= prefixed 3) "count-of-map over fresh heap strings, no intermediate")

# The same, with the map producing a MIX of heap and immediate values, so the
# guard's read discriminates rather than always passing.
(def some-heap
  (count (fn [s] (string? s))
         (map (fn [x] (if (even? x) (string "e" x) x)) [1 2 3 44])))
(assert (= some-heap 2)
        "count-of-map where only some elements become heap values")

# count-of-filter over heap strings: a dropped element must not free a kept heap
# string, and a survivor must live from the filter guard's read to the count
# guard's read in the same iteration.
(def blanks
  (count (fn [s] (empty? s)) (filter (fn [x] (string? x)) ["" "a" 1 ""])))
(assert (= blanks 2) "count-of-filter keeping heap strings")

# A tower: two dissolved intermediates, every stage reading heap values.
(def tower
  (count (fn [s] (empty? s))
         (filter (fn [v] (string? v))
                 (map (fn [x] (if (even? x) "" (string "o" x))) [1 2 3 4]))))
(assert (= tower 2) "count over a heap filter-of-map tower")

# The reorder-gate fallback with heap values: a NON-reorder-safe count predicate
# (`>` routes through `apply`) declines the composition, so the inner `filter`
# fuses alone and the outer `count` survives as a plain call over the fused loop —
# the fused loop lands as a call argument beside the count's lambda, exercising
# `lower_call`'s argument spill (call-arg-across-loop.lisp) with heap survivors.
(def fallback
  (count (fn [s] (> (length s) 1))
         (filter (fn [x] (string? x)) ["a" 1 "bb" "ccc"])))
(assert (= fallback 2)
        "count over a non-reorder-safe composition (inner-only fused)")

# The base array is bound to a Var and read AFTER the count, so the fused loop
# must not consume or free the base — it reads `coll` by index and the base
# outlives the loop.
(def base ["p" "qq" "rrr"])
(def viacount (count (fn [s] (> (length s) 1)) base))
(assert (= viacount 2) "Var-base count over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused count (not consumed)")

# The mutable-base decline still runs the stdlib op over heap elements, and the
# base stays readable afterwards.
(def @mbase @["a" "bb" "ccc"])
(assert (= (count (fn [s] (> (length s) 1)) mbase) 2)
        "mutable-base count declines to the stdlib op")
(assert (= (get mbase 2) "ccc") "the mutable base survives the declined count")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale element or mapped-value region would be reused and fault on a later pass.
(def @acc 0)
(def @i 0)
(while (< i 50)
  (let [r (count (fn [s] (string? s)) (map (fn [x] (string "v" x)) [1 22 333]))]
    (assign acc (+ acc r)))
  (assign i (+ i 1)))
(assert (= acc 150) "repeated fused counts stay sound under region-id churn")

(println "region-count-fuse-uaf: ok")
