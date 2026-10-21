(elle/epoch 15)
# audited: 2026-10-20
# `string` declares `Fresh`, so a multi-argument call leaks no region per heap argument.
# docs/impl/region/effects.md
# docs/impl/escape.md
#
# `string` (the variadic to-string / concat primitive, `prim_to_string` in
# src/primitives/convert/tostring.rs) READS its arguments and returns a FRESH
# string; it never STORES them into a longer-lived structure. Every path
# allocates in the call's own region, the one-argument string case included, so
# the declaration oracle's `Fresh` check holds and the native marks no argument
# escaping.
#
# The counter-factual is a `Mixed` declaration, which the escape analysis reads
# as "may store every heap argument" (the full mutual clique). The solver then
# emits an escape-incref on each heap argument that the native never balances,
# so every multi-argument `(string …)` call leaks ONE region per heap argument —
# measured here in regions (`arena/region-count`), deterministically.

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

# ── Correctness: concatenation yields the right value ───────────────────────
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
