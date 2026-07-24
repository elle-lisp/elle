(elle/epoch 12)
# Guardfree soundness of FOLD/REDUCE loop fusion (docs/impl/dissolution.md
# § "Fold — the scalar terminal").
#
# `(fold f init xs)` — `f` called `(f acc element)` — dissolves to an index-walk
# loop with a SCALAR accumulator: seeded by `init`, reassigned one left-fold step
# per element (`(assign acc (f acc elem))`), result is its final value. Over a
# map/filter prefix it fuses to ONE loop with no intermediate array. This fixture
# drives that path with HEAP values in three roles — heap ELEMENTS (base strings/
# structs), a heap ACCUMULATOR (the running string the fold rebuilds each step),
# and heap results threaded out — so any over-free faults at the exact access under
# --trace=guardfree rather than leaking silently. The plain-VM run also asserts the
# values, so a miscompile is loud either way.

# Heap accumulator: each step builds a FRESH heap string from the running acc and
# the element, reassigning acc. The displaced prior acc becomes dead — it must not
# be freed under the read that builds its successor, nor must the base elements.
(def joined (fold (fn [acc s] (string acc s)) "" ["a" "bb" "ccc"]))
(assert (= joined "abbccc") "fold rebuilding a heap-string accumulator")

# Left-fold order over heap values: a non-commutative combinator (bracketing)
# proves the accumulator threads in element order, and every intermediate heap acc
# survives its read.
(def bracketed (fold (fn [acc s] (string "(" acc s ")")) "" ["x" "y" "z"]))
(assert (= bracketed "(((x)y)z)")
        "left-fold order preserved through a heap accumulator")

# Heap struct elements: the combinator dereferences a heap field of each base
# element. A base struct must not be freed under the field read.
(def total (fold (fn [acc r] (+ acc (get r :v))) 0 [{:v 10} {:v 20} {:v 30}]))
(assert (= total 60) "fold reading a heap struct field per element")

# fold-of-map over heap: the map stage builds a FRESH heap string per element that
# feeds straight into the fold step — the intermediate array is gone, so the mapped
# heap value must survive from its creation to its consumption in the same loop.
(def nfold
  (fold (fn [acc s] (string acc s)) "" (map (fn [x] (string "n" x)) [1 2 3])))
(assert (= nfold "n1n2n3")
        "fold-of-map over fresh heap strings, no intermediate")

# fold-of-filter over heap strings: only survivors (the strings) reach the fold
# step; a dropped element (the integer) must not free a kept heap string, and the
# surviving heap element must live from the guard's read to the fold step's read.
(def sfold
  (fold (fn [acc s] (string acc s)) ""
        (filter (fn [s] (string? s)) ["a" 1 "b" 2 "c"])))
(assert (= sfold "abc") "fold-of-filter keeping heap strings")

# fold over a filter-of-map tower of heap values: two dissolved intermediates.
(def tower
  (fold (fn [acc s] (string acc s)) "|"
        (filter (fn [s] (string? s))
                (map (fn [x] (if (even? x) (string "e" x) x)) [1 2 3 4]))))
(assert (= tower "|e2e4") "fold over a heap filter-of-map tower")

# The reorder-gate fallback with heap values: a NON-reorder-safe prefix predicate
# (`>` routes through `apply`) declines the composition, so the inner `filter`
# fuses alone and the outer `fold` survives as a plain call over the fused loop —
# the fused loop lands as a call argument beside the fold's lambda, exercising
# `lower_call`'s argument spill (call-arg-across-loop.lisp) with heap survivors.
(def fallback
  (fold (fn [acc s] (string acc s)) ""
        (filter (fn [s] (> (length s) 1)) ["a" "bb" "c" "ddd"])))
(assert (= fallback "bbddd")
        "fold over a non-reorder-safe filter (inner-only fused)")

# The base array is bound to a Var (heap-element literal) and read AFTER the fold,
# so the fused loop must not consume/free the base — it reads `coll` by index and
# the base outlives the loop.
(def base ["p" "qq" "rrr"])
(def viafold (fold (fn [acc s] (string acc s)) "" base))
(assert (= viafold "pqqrrr") "Var-base fold over heap elements")
(assert (= (get base 0) "p")
        "the base Var survives the fused fold (not consumed)")

# Loop the whole thing so repeated fused mints/frees exercise region-id churn: a
# stale accumulator or base region would be reused and fault on a later pass.
(def @acc "")
(def @i 0)
(while (< i 50)
  (let [r (fold (fn [a s] (string a s)) "<"
                (filter (fn [s] (string? s)) ["k" 7 "q"]))]
    (assign acc r))
  (assign i (+ i 1)))
(assert (= acc "<kq") "repeated fused folds stay sound under region-id churn")

(println "region-fold-fuse-uaf: ok")
