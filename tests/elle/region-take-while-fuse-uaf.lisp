(elle/epoch 12)
# Guardfree soundness of TAKE-WHILE loop fusion (docs/impl/dissolution.md
# § "Take-while — the stage that ends the walk").
#
# `(take-while pred xs)` dissolves to an index-walk loop whose guard pushes the
# element it admits and, on the side it rejects, clears the sentinel that ends the
# run. Two things make its region accounting unlike a `filter`'s. The walk stops
# SHORT of the base, so the loop leaves with the base's later elements never read
# and the accumulator holding heap values from the elements it did read — the
# accumulator must own them past the loop, and the base must survive a walk that
# never reached its end. And the result is the accumulator itself, unfrozen, so
# what the caller holds is the very object the loop filled rather than a frozen
# copy. This fixture drives both with heap values in every role, so an over-free
# faults at the exact access under --trace=guardfree rather than leaking silently.
# The plain-VM run also asserts the values, so a miscompile is loud either way.
#
# The composition cases use single-primitive predicates (`string?`, `empty?`,
# `nil?`): a variadic comparison like `>` routes through `apply` and is not
# reorder-safe, so it would decline the composition and gauge nothing. `>` appears
# only where it is meant to — the LONE walks, which carry no reorder gate.

# Heap elements the guard reads, with a rejection that leaves later elements
# unread: the kept strings must live in the accumulator past the loop, and the
# base's unread tail must not be freed under the walk that stopped short of it.
(def leading (take-while (fn [s] (> (length s) 1)) ["bb" "ccc" "d" "eeee"]))
(assert (= (->list leading) (list "bb" "ccc")) "take-while over heap strings")
(assert (= (get leading 1) "ccc")
        "the accumulator's heap elements outlive the loop that filled them")

# Heap struct elements: the predicate dereferences a heap field per element, and
# the survivors are handed out whole.
(def rich (take-while (fn [r] (> (get r :v) 15)) [{:v 20} {:v 30} {:v 10}]))
(assert (= (get (get rich 0) :v) 20)
        "take-while reading a heap struct field per element")

# The result is the accumulator UNFROZEN, so the caller may mutate in place — the
# object the loop filled is the object handed out, holding heap elements.
(def kept (take-while (fn [s] (string? s)) ["a" "bb" 3 "c"]))
(push kept "zz")
(assert (= (->list kept) (list "a" "bb" "zz"))
        "the unfrozen result is mutable in place and keeps its heap elements")

# map-over-take-while: the take-while is the chain's innermost op, so the walk
# ends at the decision and the transform builds a FRESH heap string per survivor.
# With no intermediate array the mapped value is reachable only through the loop's
# own local until the push takes it.
(def mapped
  (map (fn [s] (string "v" s))
       (take-while (fn [x] (string? x)) ["a" "bb" 3 "c"])))
(assert (= (->list mapped) (list "va" "vbb"))
        "map over a take-while, no intermediate array")

# take-while over a MAP prefix: the walk stays exhaustive, so the transform mints a
# fresh heap string for every element — including the ones past the decision, whose
# values nothing keeps. Those must be freed without touching what the accumulator
# already holds.
(def gated
  (take-while (fn [s] (empty? s))
              (map (fn [x] (if (even? x) "" (string "o" x))) [2 4 5 6])))
(assert (= (length gated) 2)
        "take-while over a map prefix keeps the leading run of the TRANSFORMED values")

# A scalar terminal over a take-while: the survivors reach the terminal's guard
# with no array between, and the walk still ends at the take-while's decision.
(def tallied
  (count (fn [s] (string? s))
         (take-while (fn [x] (nil? (get x :gap)))
                     [{:v "a"} {:v "b"} {:gap true} {:v "c"}])))
(assert (= tallied 0) "a scalar terminal over a take-while reads the survivors")

# An EMPTY base answers with `()`, a value the loop never built — the accumulator
# is not allocated at all on that path, and the base still frees normally.
(def empty-run (take-while (fn [s] (string? s)) []))
(assert (= (type-of empty-run) :list) "an empty base answers `()`")

# The base array is bound to a Var and read AFTER the walk, so the fused loop must
# not consume or free the base — it reads `coll` by index and the base outlives it,
# including the elements the walk never reached.
(def base ["p" "qq" 3 "rrr"])
(def viatake (take-while (fn [s] (string? s)) base))
(assert (= (length viatake) 2) "Var-base take-while over heap elements")
(assert (= (get base 3) "rrr")
        "the base Var's unread tail survives the fused walk (not consumed)")

# The mutable-base decline still runs the stdlib op over heap elements, and the
# base stays readable afterwards.
(def @mbase @["a" "bb" 3])
(assert (= (length (take-while (fn [s] (string? s)) mbase)) 2)
        "mutable-base take-while declines to the stdlib op")
(assert (= (get mbase 1) "bb") "the mutable base survives the declined walk")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale element or accumulator region would be reused and fault on a later pass.
(def @acc 0)
(def @i 0)
(while (< i 50)
  (let [r (map (fn [s] (string "v" s))
               (take-while (fn [x] (string? x)) ["a" "bb" 3 "c"]))]
    (assign acc (+ acc (length r))))
  (assign i (+ i 1)))
(assert (= acc 100) "repeated fused walks stay sound under region-id churn")

(println "region-take-while-fuse-uaf: ok")
