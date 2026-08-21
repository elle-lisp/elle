(elle/epoch 12)
# tests/integration/fixtures/region-set-del-heap-member-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression ABORTS the
# process (the debug edge-table equivalence oracle panics at the drifted free,
# and under --trace=guardfree the over-free faults), and `make smoke` globs
# tests/elle/*.lisp into one shared process where an abort takes the whole
# harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_set_del_heap_member_uaf`).
#
# THE INVARIANT — a mutable @set del of a HEAP member releases the STORED
# member's region, never the caller's lookup value.
#   `(del s x)` removes the set element value-EQUAL to `x`. When the element is
#   a heap value (list/string/struct), the stored member and the lookup `x`
#   handed to `del` are two DISTINCT allocations in two DISTINCT regions that
#   merely compare equal. The add half recorded the outgoing content edge
#   `region(s) → region(stored-member)` and increfed the stored member; the
#   remove half must un-record and decref THAT SAME region. Resolving the edge
#   from the caller's `x` instead un-records an edge that was never recorded
#   (outgoing-edge accounting drift — the debug oracle detonates) and decrefs a
#   live region the caller still owns (an over-free under guardfree), while the
#   stored member's own edge and RC leak.
#
# THE CONTRAST — del by the SAME binding (`(let [m x] (add s m) (del s m))`),
# del of an INT member (no region), and @struct/@array key-removal (whose
# BTreeMap/Vec removal HANDS BACK the stored value, so the region is resolved
# from the member itself) are all clean. Only the @set value-membership remove
# loses the stored member, because a set remove yields no element to the caller.
#
# The loop drives id recycling so a drifted decref lands on a live region
# deterministically, not only when ids happen to collide.

(defn churn [reps]
  (def @c 0)
  (while (%lt c reps)
    (let [s @||]
      (add s (list 1 2))
      (add s (string "abc"))
      # del each member via a FRESH, structurally-equal lookup value — a
      # distinct allocation from the stored member.
      (del s (list 1 2))
      (del s (string "abc"))
      (assert (= (length s) 0) "set should be empty after deleting both members"))
    (assign c (%add c 1))))

(churn 4000)
(println "region-set-del-heap-member-uaf: ok")
