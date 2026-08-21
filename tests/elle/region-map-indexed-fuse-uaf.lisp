(elle/epoch 12)
# Guardfree soundness of MAP-INDEXED loop fusion (docs/impl/dissolution.md
# § "Map-indexed — the stage that carries the position").
#
# `(map-indexed f xs)` dissolves to an index-walk loop whose element statement binds
# the walk's induction variable to the function's first parameter and the element to
# its second. Two things put heap values on a path no other stage takes. The stage
# binds TWO locals per element where every other stage binds one, so the element's
# region must survive the position binding that wraps it. And the result is the
# accumulator itself, unfrozen, so what the caller holds is the very object the loop
# filled rather than a frozen copy — including where the function hands the BASE's
# own element straight through, which puts a base-owned heap value into an
# accumulator that outlives the loop. This fixture drives all of it with heap values
# in every role, so an over-free faults at the exact access under --trace=guardfree
# rather than leaking silently. The plain-VM run also asserts the values, so a
# miscompile is loud either way.
#
# The composition cases use single-primitive bodies (`string`, `string?`, `empty?`):
# a variadic comparison like `>` routes through `apply` and is not reorder-safe, so
# it would decline the composition and gauge nothing.

# The function mints a fresh heap string per element from BOTH parameters, so the
# position and the element are live in one body and the result outlives the loop.
(def tagged (map-indexed (fn [i s] (string i ":" s)) ["aa" "bb" "cc"]))
(assert (= (->list tagged) (list "0:aa" "1:bb" "2:cc"))
        "map-indexed over heap strings reads the position beside the element")
(assert (= (get tagged 2) "2:cc")
        "the accumulator's fresh heap elements outlive the loop that filled them")

# The function hands the BASE's own element through under a position test, so a
# base-owned heap value enters the accumulator: the base must still own it after the
# loop, and the accumulator must not free it.
(def picked (map-indexed (fn [i s] (if (even? i) s "-")) ["pp" "qq" "rr"]))
(assert (= (->list picked) (list "pp" "-" "rr"))
        "a base element handed straight out under its position")
(assert (= (get picked 0) "pp") "the accumulator's borrowed element stays live")

# Heap struct elements: the function dereferences a heap field per element and pairs
# it with the position, so both a read out of arg0 and a fresh aggregate ride one
# element statement.
(def rich (map-indexed (fn [i r] {:at i :v (get r :v)}) [{:v "x"} {:v "yy"}]))
(assert (= (get (get rich 1) :v) "yy")
        "a heap field read per element survives the position binding")
(assert (= (get (get rich 1) :at) 1) "and the position is the walk's own index")

# The result is the accumulator UNFROZEN, so the caller may mutate in place — the
# object the loop filled is the object handed out, holding heap elements.
(push picked "zz")
(assert (= (->list picked) (list "pp" "-" "rr" "zz"))
        "the unfrozen result is mutable in place and keeps its heap elements")

# map-over-map-indexed: no intermediate array, so the indexed transform's fresh heap
# string is reachable only through the loop's own local until the outer transform
# consumes it.
(def mapped
  (map (fn [s] (string "v" s))
       (map-indexed (fn [i s] (string i s)) ["a" "bb" "c"])))
(assert (= (->list mapped) (list "v0a" "v1bb" "v2c"))
        "map over a map-indexed, no intermediate array")

# map-indexed over a MAP prefix: the prefix mints a fresh heap string per element and
# the indexed stage consumes it under the base's own numbering, which the prefix
# preserves.
(def staged
  (map-indexed (fn [i s] (string i s)) (map (fn [x] (string "o" x)) [7 8 9])))
(assert (= (->list staged) (list "0o7" "1o8" "2o9"))
        "map-indexed over a map prefix numbers by the base walk")

# A shortening op OUTER to a map-indexed fuses whole and drops elements the indexed
# stage already minted: those must free while the survivors stay in the accumulator.
(def kept
  (filter (fn [s] (empty? s))
          (map-indexed (fn [i s] (if (even? i) "" (string i s)))
                       ["a" "b" "c" "d"])))
(assert (= (length kept) 2) "a filter outer to a map-indexed keeps the empties")
(def run
  (take-while (fn [s] (string? s))
              (map-indexed (fn [i s] (if (< i 2) (string i s) 3)) ["a" "b" "c"])))
(assert (= (->list run) (list "0a" "1b"))
        "a take-while outer to a map-indexed ends the run on a minted value")

# A scalar terminal over a map-indexed: the transformed values reach the terminal's
# guard with no array between them, so each is freed as the walk moves on.
(def tallied
  (count (fn [s] (string? s))
         (map-indexed (fn [i r] (if (even? i) (get r :v) i)) [{:v "a"} {:v "b"}])))
(assert (= tallied 1)
        "a scalar terminal over a map-indexed reads each transform")

# An EMPTY base answers with `()`, a value the loop never built — the accumulator is
# not allocated at all on that path, and the base still frees normally.
(def empty-run (map-indexed (fn [i s] (string i s)) []))
(assert (= (type-of empty-run) :list) "an empty base answers `()`")

# The base array is bound to a Var and read AFTER the walk, so the fused loop must
# not consume or free the base — it reads `coll` by index and the base outlives it.
(def base ["p" "qq" "rrr"])
(def viaindex (map-indexed (fn [i s] (string i s)) base))
(assert (= (length viaindex) 3) "Var-base map-indexed over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused walk (not consumed)")

# A shortening stage INNER to a map-indexed declines the chain — the emptiness rule
# refuses it — so the stdlib op runs over heap elements and the base stays readable.
(def declined
  (map-indexed (fn [i s] (string i s)) (filter (fn [x] (string? x)) ["a" 2 "c"])))
(assert (= (->list declined) (list "0a" "1c"))
        "a filter inner to a map-indexed declines to the stdlib op")

# The mutable-base decline runs the stdlib op over heap elements too, and the base
# stays readable afterwards.
(def @mbase @["a" "bb" "ccc"])
(assert (= (length (map-indexed (fn [i s] (string i s)) mbase)) 3)
        "mutable-base map-indexed declines to the stdlib op")
(assert (= (get mbase 1) "bb") "the mutable base survives the declined walk")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale element or accumulator region would be reused and fault on a later pass.
(def @acc 0)
(def @i 0)
(while (< i 50)
  (let [r (map (fn [s] (string "v" s))
               (map-indexed (fn [j s] (string j s)) ["a" "bb" "c"]))]
    (assign acc (+ acc (length r))))
  (assign i (+ i 1)))
(assert (= acc 150) "repeated fused walks stay sound under region-id churn")

(println "region-map-indexed-fuse-uaf: ok")
