(elle/epoch 12)
# tests/integration/fixtures/region-struct-heap-key-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression ABORTS the
# process (under --trace=guardfree the stale-key read faults; the debug
# edge-table equivalence oracle detonates on the accounting drift), and
# `make smoke` globs tests/elle/*.lisp into one shared process where an abort
# takes the whole harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_struct_heap_key_uaf`).
#
# THE INVARIANT — a struct storing a HEAP-valued key (a list/bytes/set/struct
# used as a struct key, held as `TableKey::Heap(Value)`) records the outgoing
# content edge `region(struct) → region(key)` and increfs the key's region,
# exactly as it does for a struct VALUE. The key is a cross-region reference:
# the key value is built in the caller's region and pointed at from the struct's
# region. Without that edge the key's region is reclaimed at its constructor's
# decref_point while the struct still points into it, and a later key comparison
# (binary search on `get`/`put`, or rendering) derefs the freed page.
#
# THE CONTRAST — a struct whose key value is separately BOUND live
# (`(let [k (list 1 2)] (struct k :a))`) is clean because the binding's own
# region reference keeps the key alive; an INT/keyword key (an immediate, no
# region) is clean; an ARRAY-literal key (`[1 2]`) is clean because it decomposes
# into an owned `TableKey::Array` holding no region reference. Only a heap key
# whose backing value has no other owner loses its region.
#
# The loop drives id recycling so a dropped key-region's page is reused and a
# stale comparison lands on live-but-wrong data deterministically, not only when
# ids happen to collide.

(defn churn [reps]
  (def @c 0)
  (while (%lt c reps)
    # Two compound keys built inline (no separate binding), so each key's
    # backing region is owned only through the struct — the edge under test.
    (let [s (struct (list 1 2) :a (list 3 4) :b)]
      (assert (= (get s (list 1 2)) :a) "first list key preserved")
      (assert (= (get s (list 3 4)) :b) "second list key preserved")
      # put adds a third compound key; the pre-existing keys must survive it.
      (let [s2 (put s (list 5 6) :c)]
        (assert (= (get s2 (list 1 2)) :a) "put preserves first list key")
        (assert (= (get s2 (list 3 4)) :b) "put preserves second list key")
        (assert (= (get s2 (list 5 6)) :c) "put adds third list key")))
    (assign c (%add c 1))))

(churn 4000)
(println "region-struct-heap-key-uaf: ok")
