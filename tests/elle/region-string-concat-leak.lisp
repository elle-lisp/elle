(elle/epoch 12)
# Counterfactual for the `string` concat region leak.
#
# `string` (the variadic to-string / concat primitive, `prim_to_string` in
# `src/primitives/convert.rs`) READS its arguments and returns a FRESH string; it
# never STORES them into a longer-lived structure. It was mis-declared
# `RegionEffect::Mixed`, which the escape analysis reads as "may store every heap
# argument" (the full mutual clique — docs/impl/escape.md § "Native `Mixed`/`Unknown`
# clique"). The solver then emits an escape-incref on each heap arg that the native
# never balances, so every multi-arg `(string …)` call leaks ONE region per heap
# argument — measured here in regions (`arena/region-count`), deterministically.
#
# The fix declares `string` `RegionEffect::Fresh` and makes the 1-arg-already-string
# case return a fresh copy (so the declaration oracle's Fresh check — result lives in
# the call's own region — holds on every path). A `Fresh` native marks no arg
# escaping, so the clique increfs disappear and the leak goes to zero.
#
# RED before the fix (rate ~1 region per heap arg); GREEN after.

(defn region-delta [f iters]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# Concatenations with varying heap-arg counts; the immediate `k` carries no region,
# so the leak (if any) is attributable to the string/literal heap args alone.
(defn concat-1heap [k]
  (let [s (string "a-" k)]
    0))  # 1 heap arg ("a-")
(defn concat-2heap [k]
  (let [s (string "a-" "b-" k)]
    0))  # 2 heap args
(defn concat-3lit [k]
  (let [s (string "x" "y" "z")]
    0))  # 3 heap args

# ── Correctness: concatenation still yields the right value (GREEN throughout) ──
(assert (= (string "a-" 42) "a-42") "string concatenates literal and int")
(assert (= (string "x" "y" "z") "xyz") "string concatenates literals")
(assert (= (string "only") "only") "single string arg round-trips by value")

# ── Boundedness: a `string` call leaks no region, whatever the heap-arg count ──
(let [d1 (region-delta concat-1heap 200)
      d2 (region-delta concat-2heap 200)
      d3 (region-delta concat-3lit 200)]
  (assert (%lt d1 20)
          (concat "string 1-heap-arg leak: delta=" (number->string d1)))
  (assert (%lt d2 20)
          (concat "string 2-heap-arg leak: delta=" (number->string d2)))
  (assert (%lt d3 20)
          (concat "string 3-literal leak: delta=" (number->string d3))))
