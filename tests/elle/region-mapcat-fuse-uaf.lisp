(elle/epoch 12)
# Guardfree soundness of MAPCAT loop fusion (docs/impl/dissolution.md
# § "Mapcat — the stage that fans out").
#
# `(mapcat f xs)` dissolves to an index-walk loop whose element statement binds the
# collection `f` returns and walks it with a SECOND `while`, splicing the rest of the
# pipeline inside that inner walk. Three roles put heap values on a path no other
# stage takes. The per-element collection is a fresh region born and abandoned once
# per base element, while the accumulator keeps values read out of it — so those must
# outlive the collection that carried them. The function may hand the BASE's own
# element through that collection, which routes a base-owned heap value into an
# accumulator that outlives the loop. And the result is that accumulator itself,
# unfrozen, so the caller holds the very object the loop filled rather than a frozen
# copy. This fixture drives all of it with heap values in every role, so an over-free
# faults at the exact access under --trace=guardfree rather than leaking silently.
# The plain-VM run also asserts the values, so a miscompile is loud either way.
#
# The composition cases use single-primitive bodies (`string`, `string?`, `empty?`):
# a variadic comparison like `>` routes through `apply` and is not reorder-safe, so
# it would decline the composition and gauge nothing.

# The function mints a fresh heap array holding two fresh heap strings per base
# element. That array dies as the walk moves on; what the accumulator keeps must not.
(def spread (mapcat (fn [s] [(string s "!") (string s "?")]) ["aa" "bb" "cc"]))
(assert (= (->list spread) (list "aa!" "aa?" "bb!" "bb?" "cc!" "cc?"))
        "each base element's fresh run reaches the accumulator")
(assert (= (get spread 5) "cc?")
        "the accumulator's heap elements outlive the per-element array that held them")

# The function hands the BASE's own element through the per-element array, so a
# base-owned heap value enters the accumulator: the base must still own it after the
# loop, and neither the accumulator nor the dead per-element array may free it.
(def echoed (mapcat (fn [s] [s s]) ["pp" "qq"]))
(assert (= (->list echoed) (list "pp" "pp" "qq" "qq"))
        "a base element handed through the per-element array, twice")
(assert (= (get echoed 0) "pp") "the accumulator's borrowed element stays live")

# Heap struct elements: the function dereferences a heap field per element and pairs
# it with a fresh aggregate, so a read out of arg0 and a fresh struct ride one
# per-element array.
(def rich (mapcat (fn [r] [(get r :v) {:v (get r :v)}]) [{:v "x"} {:v "yy"}]))
(assert (= (get rich 2) "yy")
        "a heap field read per element survives the inner walk")
(assert (= (get (get rich 3) :v) "yy") "and so does a fresh aggregate beside it")

# An EMPTY per-element result contributes nothing and still frees: the inner walk
# runs zero times while the base element's own region is read and released as usual.
# `->array` is a proven array producer, so this fuses, and its result's LENGTH varies
# per element — including to zero.
(def sparse (mapcat (fn [s] (->array s)) ["" "kk" ""]))
(assert (= (->list sparse) (list "k" "k"))
        "an empty per-element result enters nothing into the accumulator")

# The result is the accumulator UNFROZEN, so the caller may mutate in place — the
# object the loop filled is the object handed out, holding heap elements.
(push echoed "zz")
(assert (= (->list echoed) (list "pp" "pp" "qq" "qq" "zz"))
        "the unfrozen result is mutable in place and keeps its heap elements")

# map-over-mapcat: no flat collection between the ops, so each spliced element is
# reachable only through the inner walk's own local until the outer transform
# consumes it.
(def mapped
  (map (fn [s] (string "v" s)) (mapcat (fn [s] [s (string s s)]) ["a" "bb"])))
(assert (= (->list mapped) (list "va" "vaa" "vbb" "vbbbb"))
        "map over a mapcat, no flat collection")

# mapcat over a MAP prefix: the prefix mints a fresh heap string per element and the
# fan-out splices it into a fresh array, so a value born in one stage dies in another.
(def staged (mapcat (fn [s] [s s]) (map (fn [x] (string "o" x)) [7 8])))
(assert (= (->list staged) (list "o7" "o7" "o8" "o8"))
        "mapcat over a map prefix fans out the transformed values")

# A filter OUTER to a mapcat runs inside the inner walk and drops spliced elements
# the fan-out already minted: those must free while the survivors stay in the
# accumulator.
(def kept
  (filter (fn [s] (empty? s)) (mapcat (fn [s] ["" (string s s)]) ["a" "b"])))
(assert (= (length kept) 2) "a filter outer to a mapcat keeps the empties")

# A scalar terminal over a mapcat: each spliced element reaches the terminal's guard
# with no array between them, so each is freed as the inner walk moves on.
(def tallied
  (count (fn [s] (string? s))
         (mapcat (fn [r] [(get r :v) 3]) [{:v "a"} {:v "b"}])))
(assert (= tallied 2)
        "a scalar terminal over a mapcat reads each spliced element")

# A search over a mapcat hands a spliced heap element back out of the loop, so the
# answer must survive the per-element array it was read from.
(def found
  (find (fn [s] (empty? s)) (mapcat (fn [s] [(string s s) ""]) ["a" "b"])))
(assert (= found "") "a search over a mapcat hands a spliced element out")

# An EMPTY base answers with `()`, a value the loop never built — the accumulator is
# not allocated at all on that path, and the base still frees normally.
(def empty-run (mapcat (fn [s] [s s]) []))
(assert (= (type-of empty-run) :list) "an empty base answers `()`")

# The base array is bound to a Var and read AFTER the walk, so the fused loop must
# not consume or free the base — it reads `coll` by index and the base outlives it.
(def base ["p" "qq" "rrr"])
(def viabase (mapcat (fn [s] [s]) base))
(assert (= (length viabase) 3) "Var-base mapcat over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused walk (not consumed)")

# A list-returning function is unproven for the indexed inner walk, so the chain
# declines — the stdlib op runs over heap elements and the base stays readable.
(def declined (mapcat (fn [s] (list s s)) ["a" "bb"]))
(assert (= (->list declined) (list "a" "a" "bb" "bb"))
        "a list-returning function declines to the stdlib op")

# A shortening stage INNER to a mapcat declines the chain — the emptiness rule
# refuses it — so the stdlib op runs over heap elements.
(def inner-declined
  (mapcat (fn [s] [s s]) (filter (fn [x] (string? x)) ["a" 2 "c"])))
(assert (= (->list inner-declined) (list "a" "a" "c" "c"))
        "a filter inner to a mapcat declines to the stdlib op")

# The mutable-base arm fuses a lone mapcat, and the base stays readable afterwards.
(def @mbase @["a" "bb"])
(def mres (mapcat (fn [s] [s (string s s)]) mbase))
(assert (= (->list mres) (list "a" "aa" "bb" "bbbb"))
        "a lone mapcat fuses over a mutable @array base of heap elements")
(assert (= (get mbase 1) "bb") "the mutable base survives the fused walk")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale per-element array, element or accumulator region would be reused and fault on
# a later pass.
(def @acc 0)
(def @i 0)
(while (< i 50)
  (let [r (map (fn [s] (string "v" s))
               (mapcat (fn [s] [s (string s s)]) ["a" "bb"]))]
    (assign acc (+ acc (length r))))
  (assign i (+ i 1)))
(assert (= acc 200) "repeated fused walks stay sound under region-id churn")

(println "region-mapcat-fuse-uaf: ok")
