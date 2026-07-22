(elle/epoch 12)
# Guardfree soundness of filter loop fusion (docs/impl/dissolution.md).
#
# `(filter p xs)` over a proven immutable array with an inline non-capturing
# predicate dissolves to an index-walk loop with a GUARDED push: it binds the
# element once, tests the predicate, and pushes the element into a fresh @array
# only when the predicate passes, then freezes it. This fixture drives that path
# with HEAP element values — strings and structs — so that any over-free (a base
# element freed under the loop's read of it in the predicate or the push, or an
# accumulator member freed before the result is consumed) faults at the exact
# access under --trace=guardfree, rather than leaking silently. The plain-VM run
# also asserts the values, so a miscompile is loud either way.

# Single filter that KEEPS heap strings, read back after the loop. The predicate
# reads the heap element (its length); the survivors are the heap strings pushed
# into the accumulator.
(def kept (filter (fn [s] (> (length s) 1)) ["a" "bb" "ccc" "d"]))
(assert (= kept ["bb" "ccc"]) "single filter over heap strings")
(assert (= (get kept 1) "ccc") "fused heap survivor survives to a later read")

# Filter over heap structs, keeping by a field, then read a field back out. The
# predicate dereferences the heap element; a dropped struct must not free one that
# is kept, and vice-versa.
(def structs
  (filter (fn [r] (> (get r :v) 15))
          [{:v 10 :s "a"} {:v 20 :s "b"} {:v 30 :s "c"}]))
(assert (= (length structs) 2) "two structs kept")
(assert (= (get (get structs 0) :v) 20) "kept struct field read")
(assert (= (get (get structs 1) :s) "c") "kept struct heap field read")

# The base array is bound to a Var (heap-element literal) and read AFTER the
# filter, so the fused loop must not consume/free the base — it reads `coll` by
# index and the base outlives the loop.
(def base ["p" "qq" "rrr"])
(def viafilter (filter (fn [s] (> (length s) 1)) base))
(assert (= viafilter ["qq" "rrr"]) "Var-base fused filter over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused loop (not consumed)")

# A filter-of-filter over heap strings: the element is pushed through TWO nested
# guards. Both predicates are reorder-safe type tests, so the composition fuses;
# the point here is the guarded push of a HEAP value through nested `if`s.
(def nested
  (filter (fn [x] (string? x)) (filter (fn [x] (string? x)) ["v1" "v2" "v3"])))
(assert (= nested ["v1" "v2" "v3"]) "filter-of-filter over heap values")
(assert (= (get nested 0) "v1") "nested-guard heap survivor survives")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale accumulator or base region would be reused and fault on a later pass.
(def @acc "")
(def @i 0)
(while (< i 50)
  (let [r (filter (fn [s] (> (length s) 1)) ["z" "keep" "q"])]
    (assign acc (get r 0)))
  (assign i (+ i 1)))
(assert (= acc "keep")
        "repeated fused filter loops stay sound under region-id churn")

(println "region-filter-fuse-uaf: ok")
