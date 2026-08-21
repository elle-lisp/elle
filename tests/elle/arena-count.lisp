(elle/epoch 12)
# arena-count.lisp — arena/count must reflect REAL reclamation
#
# arena/count returns the live object count summed across active regions
# (FiberHeap::visible_len → RegionStore::total_obj_count), NOT a running
# alloc-counter that only moves on the decref/free_region paths.
#
# Why this matters: a scope-reclaimed allocation in a loop — e.g. a let
# body, or a closure capturing a heap value, freed by region-exit each
# iteration — recycles its pages without necessarily flowing through
# `decref_region`. The old running counter missed those reclamations and
# over-reported a phantom per-iteration "leak" (nondeterministically,
# depending on which reclamation path won the race) that peak RSS flatly
# contradicted. Reading the live per-region object counts is both correct
# and deterministic.
#
# These loops are all genuinely bounded (RSS-confirmed flat): arena/count
# must report that, every run, in every execution mode.

(defn capture-loop [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [p (%pair i i)  # a heap value …
          f (fn [] p)]
      (f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (capture-loop 100)
      d10k (capture-loop 10000)]
  (assert (and (%lt d100 10) (%lt d10k 10))
          (string "capture loop must read bounded: d100=" d100 " d10k=" d10k)))

(defn let-struct-loop [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [s {:v i}]
      s)
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d100 (let-struct-loop 100)
      d10k (let-struct-loop 10000)]
  (assert (and (%lt d100 10) (%lt d10k 10))
          (string "let-struct loop must read bounded: d100=" d100 " d10k=" d10k)))

(println "arena-count: all tests passed")
