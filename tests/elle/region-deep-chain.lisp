(elle/epoch 12)
# tests/elle/region-deep-chain.lisp — freeing a deep region chain must not
# overflow the native stack.
#
# Counterfactual for a stack overflow in the region-store free cascade
# (src/value/fiberheap/regionstore/free.rs). When a region is reclaimed, its
# genuinely-Shared cross-region references form a "frontier" that must itself be
# decref'd; a frontier reference that reaches rc 0 is another region to free,
# whose frontier feeds the same process. When that cascade is driven by native
# recursion (free → decref → free → …), a *chain* of N regions where each holds
# a reference to the next spends one native stack frame per link. At a few
# thousand links the stack overflows and the process aborts with SIGABRT.
# The Rust-side pin is `deep_cascade_chain_does_not_overflow_stack`
# (regionstore::tests); this file is the end-to-end Elle reproduction.
#
# Three shapes build such a chain, each well past the recursive crash threshold:
#   1. nested arrays  — region k holds `@[region k+1]`
#   2. a cons/pair list — each `%pair` cell references the tail
#   3. `(apply concat <thousands-of-chunks>)` — the real-world trigger behind
#      the h2 stress loops (a large body assembled from many small chunks).
# Pre-fix any one of them aborts the process the moment its head is dropped.

(def depth 4000)

# 1. Deep nested-array chain, then drop it. Rebinding `acc` to a fresh empty
#    array releases the whole chain at once — the head free must cascade all
#    `depth` links iteratively.
(def @acc @[])
(def @i 0)
(while (< i depth)
  (assign acc @[acc])
  (assign i (+ i 1)))
(assign acc @[])

# 2. Deep pair-list chain, then drop it.
(def @p ())
(def @j 0)
(while (< j depth)
  (assign p (%pair j p))
  (assign j (+ j 1)))
(assign p ())

# 3. concat over thousands of chunks: the accumulator's reclamation walks the
#    same cross-region frontier the recursive cascade choked on.
(def @chunks @[])
(def @k 0)
(while (< k 4000)
  (push chunks (bytes 0 1 2 3 4 5 6 7))
  (assign k (+ k 1)))
(def blob (apply concat chunks))
(assert (= (length blob) 32000)
        "concat over 4000 chunks yields the summed length")

# Reaching here at all is the assertion: a recursive free cascade never returns
# from dropping the chains above — it aborts the process first.
(println "region-deep-chain ok: freed " depth "-deep chains + 4000-chunk concat")
