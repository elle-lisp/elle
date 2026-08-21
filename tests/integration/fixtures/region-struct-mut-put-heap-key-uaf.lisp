(elle/epoch 12)
# tests/integration/fixtures/region-struct-mut-put-heap-key-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression ABORTS the
# process (the debug edge-table equivalence oracle detonates on the accounting
# drift, and under --trace=guardfree the stale-key read faults), and `make smoke`
# globs tests/elle/*.lisp into one shared process where an abort takes the whole
# harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_struct_mut_put_heap_key_uaf`).
#
# THE INVARIANT — a mutable @struct `put` that ADDS a heap-valued key (a
# fiber/closure/list used as a struct key) records the outgoing content edge
# `region(struct) → region(key)` and increfs the key's region, and a `del` of that
# key un-records and decrefs it — symmetric with the alloc-scan that does it for a
# struct BUILT with keys (`find_object_cross_refs`'s `LStructMut` arm walks keys).
#   The alloc-scan handles keys present at construction, but an in-place `put`
#   adds a key AFTER allocation, so the key edge must be recorded by the store
#   funnel (`struct_put_with_rebind`) — the value-side records only the value.
#   Without it the free-time content scan finds the key while the recorded edge
#   table does not (a missed store-funnel edge the equivalence oracle detonates
#   on), and the key's region is freed while the struct still points into it.
#
# THE CONTRAST — the IMMUTABLE `(put s k v)` twin (a fresh copy, whose alloc-scan
# records every key) is covered by `region-struct-heap-key-uaf.lisp`; an
# INT/keyword key (an immediate, no region) is clean.
#
# The loop drives id recycling so a dropped key-region's page is reused and a
# drifted free lands deterministically, not only when ids happen to collide.

(defn churn [reps]
  (def @c 0)
  (while (%lt c reps)
    # fiber and closure keys added by in-place put, then read and removed.
    (let [fib (fiber/new (fn () 1) 0)
          clo (fn () 2)]
      (let [t @{}]
        (put t fib :fiber-data)
        (put t clo :closure-data)
        (assert (= (get t fib) :fiber-data) "fiber key preserved")
        (assert (= (get t clo) :closure-data) "closure key preserved")
        # overwrite a heap key's value (rebind — must not double-record the key)
        (put t fib :fiber-data-2)
        (assert (= (get t fib) :fiber-data-2) "fiber key rebind")
        # del a heap key (must un-record + decref the stored key edge)
        (del t clo)
        (assert (= (has? t clo) false) "closure key removed")))
    # a compound (list) key added by in-place put, then removed
    (let [t @{}]
      (put t (list c (%add c 1)) :v)
      (assert (= (get t (list c (%add c 1))) :v) "compound key preserved")
      (del t (list c (%add c 1)))
      (assert (= (has? t (list c (%add c 1))) false) "compound key removed"))
    (assign c (%add c 1))))

(churn 4000)
(println "region-struct-mut-put-heap-key-uaf: ok")
