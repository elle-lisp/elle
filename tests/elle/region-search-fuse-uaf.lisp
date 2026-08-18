(elle/epoch 12)
# Guardfree soundness of SEARCH loop fusion (docs/impl/dissolution.md
# § "Search — the terminal that stops early").
#
# `any?`, `all?`, `find` and `find-index` each dissolve to an index-walk loop whose
# last stage is the predicate's guard and whose base case writes a scalar answer and
# clears the `more` sentinel the loop condition reads. Two roles put heap values on
# that path. The base's heap elements must stay live for the guard that reads them,
# on the deciding iteration and on every earlier one. And `find` is the only fused
# terminal whose accumulator holds a value it did NOT allocate: it records a base
# element and hands it out of the loop, past the base binding's own demise, so the
# result must outlive the walk that found it. This fixture drives both with heap
# values, so an over-free faults at the exact access under --trace=guardfree rather
# than leaking silently. The plain-VM run also asserts the values, so a miscompile is
# loud either way.
#
# A search fuses only as a lone op, so every case here is one; the predicates are
# free to be variadic (`>` routes through `apply`) because a lone op carries no
# reorder gate.

# Heap elements read by the guard: a base string must not be freed under the
# predicate that measures it, nor under a later iteration's read.
(assert (= (any? (fn [s] (> (length s) 2)) ["a" "bb" "ccc" "d"]) true)
        "any? over heap-string elements")
(assert (= (all? (fn [s] (> (length s) 0)) ["a" "bb" "ccc"]) true)
        "all? reading every heap element")
(assert (= (all? (fn [s] (> (length s) 1)) ["aa" "b" "ccc"]) false)
        "all? rejected by a heap element mid-walk")

# Heap struct elements: the predicate dereferences a heap field per element.
(assert (= (find-index (fn [r] (> (get r :v) 15)) [{:v 10} {:v 20} {:v 30}]) 1)
        "find-index reading a heap struct field per element")

# `find` hands a base element OUT of the loop. The recorded value is the base's
# own heap string, so the loop's `coll` binding must not free it under the result.
(def found (find (fn [s] (> (length s) 2)) ["a" "bb" "ccc" "d"]))
(assert (= found "ccc") "find records the base's heap element as its answer")
(assert (= (length found) 3)
        "the recorded heap element is readable after the loop")

# The same, with a heap STRUCT answer whose field is read after the walk.
(def rec (find (fn [r] (> (get r :v) 15)) [{:v 10} {:v 20} {:v 30}]))
(assert (= (get rec :v) 20) "a heap struct answer survives the fused search")

# A base bound to a Var and read AFTER the search: the fused loop reads `coll` by
# index and must not consume or free the base.
(def base ["p" "qq" "rrr"])
(def hit (find (fn [s] (> (length s) 1)) base))
(assert (= hit "qq") "Var-base find over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused search (not consumed)")
(assert (= (any? (fn [s] (> (length s) 2)) base) true)
        "a second search over the same base")

# An undecided walk reads every element and answers the seed, with the whole base
# still live at the end.
(assert (= (find (fn [s] (> (length s) 9)) base) nil)
        "an undecided find answers nil")
(assert (= (get base 2) "rrr") "the base outlives an undecided walk")

# Heap values MINTED by the predicate on each element: a per-element temporary must
# not be freed under its own read, and must not outlive the iteration that made it.
(assert (= (any? (fn [s] (= (string "v" s) "vbb")) ["a" "bb" "ccc"]) true)
        "a predicate minting a fresh heap value per element")

# The mutable-base decline still runs the stdlib op over heap elements, and the
# base stays readable afterwards.
(def @mbase @["a" "bb" "ccc"])
(assert (= (find (fn [s] (> (length s) 1)) mbase) "bb")
        "mutable-base find declines to the stdlib op")
(assert (= (get mbase 2) "ccc") "the mutable base survives the declined search")

# Loop the searches so repeated fused mints/frees exercise region-id churn: a stale
# element region would be reused and fault on a later pass. The `find` result is
# rebound each trip, so the previous answer's region is released under the next.
(def @acc 0)
(def @i 0)
(while (< i 50)
  (let [r (find (fn [s] (> (length s) 2)) ["a" "bb" "ccc" "d"])]
    (when (= (length r) 3) (assign acc (+ acc 1))))
  (assign i (+ i 1)))
(assert (= acc 50) "repeated fused searches stay sound under region-id churn")

(println "region-search-fuse-uaf: ok")
