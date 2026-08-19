(elle/epoch 12)
# Guardfree soundness of DROP-WHILE loop fusion (docs/impl/dissolution.md
# § "Drop-while — the stage that starts late").
#
# `(drop-while pred xs)` dissolves to an index-walk loop whose guard clears a
# `dropping` flag at the first element its predicate rejects, after which every
# element is pushed. Two things make its region accounting unlike a `take-while`'s.
# The accumulator fills from the TAIL of the base, so the elements the loop read
# first — the leading run the predicate consumed and discarded — must be freed
# while the base still owns the ones the accumulator now holds. And the predicate
# stops running at the decision while the walk does not, so every later element is
# read and pushed by a path that never binds it to the predicate's parameter. The
# result is the accumulator itself, unfrozen, so what the caller holds is the very
# object the loop filled rather than a frozen copy. This fixture drives all of it
# with heap values in every role, so an over-free faults at the exact access under
# --trace=guardfree rather than leaking silently. The plain-VM run also asserts the
# values, so a miscompile is loud either way.
#
# The composition cases use single-primitive predicates (`string?`, `empty?`,
# `nil?`): a variadic comparison like `>` routes through `apply` and is not
# reorder-safe, so it would decline the composition and gauge nothing. `>` appears
# only where it is meant to — the LONE walks, which carry no reorder gate.

# Heap elements the guard reads, then discards: the leading run's strings are read
# by the predicate and enter nothing, while the tail's must live in the accumulator
# past the loop.
(def tail (drop-while (fn [s] (> (length s) 1)) ["bb" "ccc" "d" "eeee"]))
(assert (= (->list tail) (list "d" "eeee")) "drop-while over heap strings")
(assert (= (get tail 1) "eeee")
        "the accumulator's heap elements outlive the loop that filled them")

# Heap struct elements: the predicate dereferences a heap field per element of the
# leading run, and the elements past it are handed out whole without that read.
(def rich
  (drop-while (fn [r] (> (get r :v) 15)) [{:v 20} {:v 30} {:v 10} {:v 40}]))
(assert (= (get (get rich 0) :v) 10)
        "drop-while reading a heap struct field per element of the leading run")
(assert (= (get (get rich 1) :v) 40)
        "an element past the decision is pushed without reaching the predicate")

# The result is the accumulator UNFROZEN, so the caller may mutate in place — the
# object the loop filled is the object handed out, holding heap elements.
(def kept (drop-while (fn [s] (string? s)) ["a" "bb" 3 "c"]))
(push kept "zz")
(assert (= (->list kept) (list 3 "c" "zz"))
        "the unfrozen result is mutable in place and keeps its heap elements")

# map-over-drop-while: no intermediate array, so the transform's fresh heap string
# is reachable only through the loop's own local until the push takes it. The
# transform runs on the passed-on elements alone.
(def mapped
  (map (fn [s] (string "v" s))
       (drop-while (fn [x] (string? x)) ["a" "bb" 3 "c"])))
(assert (= (->list mapped) (list "v3" "vc"))
        "map over a drop-while, no intermediate array")

# drop-while over a MAP prefix: the transform mints a fresh heap string for every
# element, including the leading run's, whose values nothing keeps. Those must be
# freed without touching what the accumulator already holds.
(def gated
  (drop-while (fn [s] (empty? s))
              (map (fn [x] (if (even? x) "" (string "o" x))) [2 4 5 6])))
(assert (= (length gated) 2)
        "drop-while over a map prefix passes on the TRANSFORMED values")
(assert (= (get gated 0) "o5") "the deciding element is the first one kept")

# A scalar terminal over a drop-while: the passed-on elements reach the terminal's
# guard with no array between them.
(def tallied
  (count (fn [s] (string? s))
         (drop-while (fn [x] (nil? (get x :gap)))
                     [{:v "a"} {:v "b"} {:gap true} {:v "c"}])))
(assert (= tallied 0)
        "a scalar terminal over a drop-while reads what was passed on")

# A `find-index` over a drop-while answers a RENUMBERED position, so the loop
# carries a survivor count beside the index walk while heap elements flow through.
(def pos
  (find-index (fn [s] (string? s))
              (drop-while (fn [x] (number? x)) [1 2 "cc" 3])))
(assert (= pos 0) "the survivor count answers the position in the passed-on run")

# An EMPTY base answers with `()`, a value the loop never built — the accumulator
# is not allocated at all on that path, and the base still frees normally.
(def empty-run (drop-while (fn [s] (string? s)) []))
(assert (= (type-of empty-run) :list) "an empty base answers `()`")

# A walk that drops EVERYTHING reads every element into the predicate and keeps
# none: the accumulator is allocated, stays empty, and is handed out.
(def all-dropped (drop-while (fn [s] (string? s)) ["a" "bb" "ccc"]))
(assert (= (length all-dropped) 0) "an undecided walk keeps nothing")
(assert (= (type-of all-dropped) :@array)
        "and still answers with the accumulator")

# The base array is bound to a Var and read AFTER the walk, so the fused loop must
# not consume or free the base — it reads `coll` by index and the base outlives it,
# including the leading run the accumulator never took.
(def base ["p" "qq" 3 "rrr"])
(def viadrop (drop-while (fn [s] (string? s)) base))
(assert (= (length viadrop) 2) "Var-base drop-while over heap elements")
(assert (= (get base 0) "p")
        "the base Var's dropped head survives the fused walk (not consumed)")

# The mutable-base decline still runs the stdlib op over heap elements, and the
# base stays readable afterwards.
(def @mbase @["a" "bb" 3])
(assert (= (length (drop-while (fn [s] (string? s)) mbase)) 1)
        "mutable-base drop-while declines to the stdlib op")
(assert (= (get mbase 1) "bb") "the mutable base survives the declined walk")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale element or accumulator region would be reused and fault on a later pass.
(def @acc 0)
(def @i 0)
(while (< i 50)
  (let [r (map (fn [s] (string "v" s))
               (drop-while (fn [x] (string? x)) ["a" "bb" 3 "c"]))]
    (assign acc (+ acc (length r))))
  (assign i (+ i 1)))
(assert (= acc 100) "repeated fused walks stay sound under region-id churn")

(println "region-drop-while-fuse-uaf: ok")
